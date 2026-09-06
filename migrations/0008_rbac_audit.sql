-- 0008_rbac_audit：使用者、角色、權限、登入紀錄、稽核、主管授權、outbox。
--
-- 稽核是「補做會很痛」清單裡的第 2 項，而且是不可逆的：事後補表只能從今天開始有資料，
-- 過去半年發生過什麼永遠查不出來。餐飲業的弊案多半是連續性的（每天免一單、
-- 每天作廢一筆），沒有歷史就沒有辦法發現模式。

CREATE TABLE users (
  id            TEXT NOT NULL PRIMARY KEY,
  store_id      TEXT NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
  code          TEXT NOT NULL,
  name          TEXT NOT NULL,
  email         TEXT,
  -- 4 碼 PIN 的熵極低，必須配合「同 code 連續失敗 N 次鎖 N 分鐘」（查 login_records）。
  -- 雜湊仍用 argon2 —— 便宜歸便宜，資料庫外洩時不該直接看到 PIN。
  pin_hash      TEXT,
  password_hash TEXT,
  is_active     INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL,
  deleted_at    TEXT
);
CREATE UNIQUE INDEX uq_users_code ON users(store_id, code) WHERE deleted_at IS NULL;
-- 用 lower() 函式索引而不是 COLLATE NOCASE：後者是 SQLite 專有語法。
-- 對應的查詢也必須寫 lower(email) 才會命中這個索引。
CREATE UNIQUE INDEX uq_users_email ON users(lower(email)) WHERE email IS NOT NULL AND deleted_at IS NULL;

CREATE TABLE roles (
  id           TEXT NOT NULL PRIMARY KEY,
  name         TEXT NOT NULL,
  display_name TEXT NOT NULL,
  description  TEXT,
  is_system    INTEGER NOT NULL DEFAULT 0 CHECK (is_system IN (0, 1)),
  sort_order   INTEGER NOT NULL DEFAULT 0,
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL
);
CREATE UNIQUE INDEX uq_roles_name ON roles(name);

CREATE TABLE permissions (
  id          TEXT NOT NULL PRIMARY KEY,
  code        TEXT NOT NULL,
  name        TEXT NOT NULL,
  group_code  TEXT NOT NULL,
  group_name  TEXT NOT NULL,
  description TEXT,
  sort_order  INTEGER NOT NULL DEFAULT 0,
  created_at  TEXT NOT NULL
);
CREATE UNIQUE INDEX uq_permissions_code ON permissions(code);

CREATE TABLE role_permissions (
  role_id       TEXT NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
  permission_id TEXT NOT NULL REFERENCES permissions(id) ON DELETE CASCADE,
  created_at    TEXT NOT NULL,
  PRIMARY KEY (role_id, permission_id)
);

CREATE TABLE user_roles (
  user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  role_id    TEXT NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
  created_at TEXT NOT NULL,
  PRIMARY KEY (user_id, role_id)
);

-- 登入紀錄。
-- user_code 是 NOT NULL 而 user_id 可空：打錯員工編號的失敗嘗試也要留下來，
-- 那正是「有人在試別人的 PIN」的訊號。
CREATE TABLE login_records (
  id             TEXT NOT NULL PRIMARY KEY,
  user_code      TEXT NOT NULL,
  user_id        TEXT REFERENCES users(id) ON DELETE SET NULL,
  terminal_id    TEXT REFERENCES terminals(id) ON DELETE SET NULL,
  provider       TEXT NOT NULL DEFAULT 'pin' CHECK (provider IN ('pin', 'password', 'card')),
  status         TEXT NOT NULL CHECK (status IN ('success', 'fail', 'locked')),
  failure_reason TEXT,
  ip_address     TEXT,
  created_at     TEXT NOT NULL
);
CREATE INDEX idx_login_records_code ON login_records(user_code, created_at);
CREATE INDEX idx_login_records_status ON login_records(status, created_at);

-- 稽核。
--
-- 與 kanban 的 audit-log-service 有一個刻意的分歧：那邊用 try/catch 吞掉稽核寫入失敗，
-- 因為對看板系統來說「功能可用」比「紀錄完整」重要。
-- **POS 反過來：金額相關的稽核（改價 / 作廢 / 折讓 / 退款）寫入失敗必須 rollback
-- 整筆交易。** 防弊優先於可用性 —— 一筆沒有紀錄的免單，比一次結帳失敗糟糕得多。
CREATE TABLE audit_logs (
  id            TEXT NOT NULL PRIMARY KEY,
  actor_id      TEXT,
  actor_code    TEXT,
  actor_name    TEXT,
  entity_type   TEXT NOT NULL,
  entity_id     TEXT NOT NULL,
  action        TEXT NOT NULL,
  old_value     TEXT,
  new_value     TEXT,
  -- ★ 金額影響（整數元，可負）。這是整張表最有價值的欄位：
  --   「這個月誰總共免掉了多少錢」一個 SUM 就查得出來。
  amount_delta  INTEGER,
  reason_id     TEXT REFERENCES reason_codes(id),
  approved_by   TEXT,
  terminal_id   TEXT,
  shift_id      TEXT,
  business_date TEXT,
  ip_address    TEXT,
  created_at    TEXT NOT NULL
);
CREATE INDEX idx_audit_logs_entity ON audit_logs(entity_type, entity_id);
CREATE INDEX idx_audit_logs_actor ON audit_logs(actor_code, created_at);
CREATE INDEX idx_audit_logs_action ON audit_logs(action, business_date);
CREATE INDEX idx_audit_logs_money ON audit_logs(business_date, actor_code) WHERE amount_delta IS NOT NULL;

-- 主管授權。每一次敏感動作留一筆，含當下用了哪種身分驗證方式。
CREATE TABLE approvals (
  id            TEXT NOT NULL PRIMARY KEY,
  action_code   TEXT NOT NULL,
  ref_type      TEXT NOT NULL,
  ref_id        TEXT NOT NULL,
  amount        INTEGER,
  reason_id     TEXT REFERENCES reason_codes(id),
  note          TEXT,
  requested_by  TEXT NOT NULL,
  approved_by   TEXT NOT NULL,
  auth_method   TEXT NOT NULL DEFAULT 'pin' CHECK (auth_method IN ('pin', 'password', 'card')),
  terminal_id   TEXT,
  shift_id      TEXT,
  business_date TEXT NOT NULL,
  approved_at   TEXT NOT NULL,
  created_at    TEXT NOT NULL
);
CREATE INDEX idx_approvals_ref ON approvals(ref_type, ref_id);
CREATE INDEX idx_approvals_date ON approvals(business_date, action_code);

-- Outbox：所有需要外部 I/O 的動作。
--
-- 存在的唯一理由：**交易內嚴禁任何外部 I/O**。寫入池只有一條連線，
-- 一個卡住的 TCP 連線會讓全店的寫入排隊。所以「要印一張單」「要上傳一張發票」
-- 在交易內只是寫一列，真正的動作由背景 worker 做並重試。
CREATE TABLE outbox (
  id              TEXT NOT NULL PRIMARY KEY,
  kind            TEXT NOT NULL,
  payload_json    TEXT NOT NULL,
  status          TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'running', 'done', 'failed', 'dead')),
  attempts        INTEGER NOT NULL DEFAULT 0,
  next_attempt_at TEXT NOT NULL,
  last_error      TEXT,
  business_date   TEXT,
  created_at      TEXT NOT NULL,
  updated_at      TEXT NOT NULL,
  done_at         TEXT
);
CREATE INDEX idx_outbox_ready ON outbox(status, next_attempt_at);
CREATE INDEX idx_outbox_kind ON outbox(kind, status);

-- 稽核軌跡的第二份保險：append-only 的 NDJSON journal 由背景 worker 寫檔並 fsync，
-- 這張表是它在資料庫內的來源。資料庫真的壞掉時，靠 journal 檔重建當日營業。
CREATE TABLE journal (
  id            TEXT NOT NULL PRIMARY KEY,
  business_date TEXT NOT NULL,
  kind          TEXT NOT NULL,
  payload_json  TEXT NOT NULL,
  flushed_at    TEXT,
  created_at    TEXT NOT NULL
);
CREATE INDEX idx_journal_unflushed ON journal(created_at) WHERE flushed_at IS NULL;
