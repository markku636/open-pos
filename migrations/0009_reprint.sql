-- 補印收據。
--
-- 補印的是**那一張帳單**的收據，而分帳之後一張訂單會有好幾張帳單
-- （$150 分四份就是四張），只靠 order_id 找不回客人手上是哪一份。
--
-- 收據原稿存在帳單上，而不是去翻列印佇列：
--
-- 1. 還沒接印表機時照樣結得了帳（那些單留在 outbox），但客人還是要收據 ——
--    補印不該因為當初沒印出來就辦不到。
-- 2. print_jobs 是操作紀錄，遲早要按日期清掉；帳單不會。
--
-- 重送的是**當初那一份的快照**，不是重新排版。這一點與 v1.0 早期的想法相反，
-- 而理由正是分帳：一張訂單的第二份帳單無法從訂單重建。快照同時也更誠實 ——
-- 補印本來就該是「再給你一張一樣的」。
ALTER TABLE bills ADD COLUMN receipt_doc TEXT;

-- 列印佇列也記一下是哪一張帳單。診斷「這張單印到哪去了」時要用。
ALTER TABLE print_jobs ADD COLUMN bill_id TEXT REFERENCES bills(id) ON DELETE SET NULL;
CREATE INDEX idx_print_jobs_bill ON print_jobs(bill_id);
