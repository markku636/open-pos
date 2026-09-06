-- 電子金流。
--
-- # 為什麼要多兩張表，而不是把欄位加在 payment_methods 上
--
-- 「付款方式」是**收銀畫面上的一顆按鈕**（現金、信用卡、LINE Pay），
-- 「金流」是**背後那條線**（藍新、LINE Pay 商家帳號）。兩者不是一對一：
-- 同一個金流商可以承接好幾種付款方式，而同一種付款方式在不同分店可能走
-- 不同的金流商。把憑證塞進按鈕的定義裡，換金流商就要改按鈕。
--
-- # gateway_transactions 存在的唯一理由：對帳
--
-- 電子金流最貴的失敗不是「刷卡失敗」—— 那個收銀員當場就看得到。真正貴的是
-- **「請求送出去了，但回應沒收到」**：錢在金流商那邊扣了，POS 這邊卻不知道。
-- 沒有這張表就沒有任何線索可以回頭查，而客人已經走了。
--
-- 所以每一次對外請求在**送出之前**就先寫一列（status='pending'），
-- 回應回來才改狀態。斷線、當機、超時之後留下的那些 pending/unknown 列，
-- 就是隔天早上要拿去跟金流商後台對的清單。

CREATE TABLE payment_gateways (
  id                TEXT NOT NULL PRIMARY KEY,
  store_id          TEXT NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
  -- manual = 不串接，收銀員自己把終端機上的授權碼抄進來（現在的行為）。
  provider          TEXT NOT NULL CHECK (provider IN ('manual', 'linepay', 'newebpay')),
  display_name      TEXT NOT NULL,
  -- 這條線承接哪一顆付款按鈕。
  payment_method_id TEXT REFERENCES payment_methods(id) ON DELETE SET NULL,
  -- 測試環境。**預設是 1** —— 一個預設連到正式環境的設定頁遲早會有人
  -- 拿真的信用卡去測。
  is_sandbox        INTEGER NOT NULL DEFAULT 1 CHECK (is_sandbox IN (0, 1)),
  is_active         INTEGER NOT NULL DEFAULT 0 CHECK (is_active IN (0, 1)),
  -- 憑證（JSON）。**這一欄跟資料庫一樣敏感** —— 診斷包與 log 一律不含它，
  -- 而備份出去的 .db 檔要當成含有金流憑證的檔案來保管。
  credentials_json  TEXT,
  config_json       TEXT,
  created_at        TEXT NOT NULL,
  updated_at        TEXT NOT NULL,
  deleted_at        TEXT
);
CREATE INDEX idx_payment_gateways_store ON payment_gateways(store_id) WHERE deleted_at IS NULL;

CREATE TABLE gateway_transactions (
  id                TEXT NOT NULL PRIMARY KEY,
  gateway_id        TEXT NOT NULL REFERENCES payment_gateways(id) ON DELETE RESTRICT,
  -- 成功之後才會有 payment；請求送出去的當下還沒有。
  payment_id        TEXT REFERENCES payments(id) ON DELETE SET NULL,
  order_id          TEXT REFERENCES orders(id) ON DELETE SET NULL,
  business_date     TEXT NOT NULL,
  shift_id          TEXT,
  -- 我方的交易編號，送給金流商當訂單號。重試時**沿用同一個** ——
  -- 那是「不要扣兩次」唯一的保證，而金流商認的是它。
  merchant_trade_no TEXT NOT NULL,
  kind              TEXT NOT NULL CHECK (kind IN ('payment', 'refund')),
  amount            INTEGER NOT NULL,
  -- unknown 是這張表最重要的一個狀態：**送出去了但不知道結果**。
  -- 它不是錯誤，是「要去對帳」。把它併進 failed 就會讓錢消失得無聲無息。
  status            TEXT NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending', 'authorized', 'captured',
                                      'failed', 'cancelled', 'unknown')),
  gateway_trade_no  TEXT,
  request_json      TEXT,
  response_json     TEXT,
  error_message     TEXT,
  attempts          INTEGER NOT NULL DEFAULT 0,
  created_at        TEXT NOT NULL,
  updated_at        TEXT NOT NULL,
  done_at           TEXT
);
-- 同一個交易編號只能有一列。這是冪等的最後一道保險。
CREATE UNIQUE INDEX uq_gateway_tx_no ON gateway_transactions(gateway_id, merchant_trade_no);
CREATE INDEX idx_gateway_tx_date ON gateway_transactions(business_date, status);
CREATE INDEX idx_gateway_tx_payment ON gateway_transactions(payment_id);
