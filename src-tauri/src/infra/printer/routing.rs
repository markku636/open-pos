//! 「這一單要印到哪幾台機器上」。
//!
//! # 三層間接，不是兩層
//!
//! 品項/分類 → **出單分區** → 印表機。
//!
//! 把品項直接綁印表機是新手 POS 最常見的設計錯誤：換一台機器、加一台備援機，
//! 就要去改幾百筆菜單。分區這一層讓「飲料吧」是一個穩定的概念，
//! 底下綁哪台機器純粹是設定問題。
//!
//! # 為什麼是純函式
//!
//! `plan_jobs` 不碰資料庫、不碰時鐘、不碰網路。所有輸入都由呼叫端讀好餵進來，
//! 所以「三個分區各印各的、飲料吧掛了改印櫃檯、加點只印新增的那幾行」
//! 這些真正容易錯的規則，全部可以在單元測試裡窮舉。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::receipt::templates::TicketReason;

/// 出單分區。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Station {
    pub id: String,
    pub name: String,
    /// kitchen / drink / receipt / label
    pub template: String,
    /// 每品項一張。燒烤與貼杯標籤的站常這樣（一張單跟著一份餐走）。
    pub split_per_item: bool,
    pub sort_order: i64,
}

/// 分區綁到哪一台機器。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub station_id: String,
    pub printer_id: String,
    /// 數字小的優先。
    pub priority: i64,
    /// failover：只印第一台，掛了才換下一台。
    /// always：每一台都印（「熱炒區一份、櫃檯留底一份」）。
    pub mode: BindingMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingMode {
    Failover,
    Always,
}

/// 一行要出的品項，加上它屬於哪一區。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutableLine {
    pub line_id: String,
    /// 品項自己的分區。None 代表跟著分類走。
    pub station_id: Option<String>,
    /// 分類的預設分區。None 代表沒設 —— 那就落到 fallback。
    pub category_station_id: Option<String>,
}

impl RoutableLine {
    /// 三層預設：品項 > 分類 > 沒有。
    ///
    /// 只放品項層的話，新增 200 個飲料要設 200 次；只放分類層則店長設定不出來
    /// 「這一款熱飲要走熱飲區」。兩層都要。
    pub fn resolve_station(&self) -> Option<&str> {
        self.station_id
            .as_deref()
            .or(self.category_station_id.as_deref())
    }
}

/// 排好的一張單。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedJob {
    pub printer_id: String,
    pub station_id: Option<String>,
    pub station_name: Option<String>,
    /// 這張單上要印哪幾行（依 `RoutableLine::line_id`）。
    pub line_ids: Vec<String>,
    pub reason: TicketReason,
    /// 這一台是不是備援位置。true 時單頭要印「※ ○○區代印 ※」——
    /// 不然櫃檯印出一張熱炒單，沒有人知道那是要拿去哪裡的。
    pub is_backup: bool,
}

/// 沒有任何分區能收的品項要怎麼辦。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fallback<'a> {
    /// 印到這一台（通常是櫃檯那台）。
    Printer(&'a str),
    /// 不印。
    ///
    /// ⚠️ 只有在呼叫端會**另外**提醒使用者時才可以用這個。
    /// 「系統知道有東西沒印但沒告訴任何人」是 POS 最常見客訴的技術根因。
    Drop,
}

/// 把一張訂單排成幾張要印的單。
///
/// * `lines` —— 這一次要出的行（加點時只給新增的那幾行）
/// * `stations` —— 全部啟用中的分區
/// * `bindings` —— 分區到印表機的綁定
/// * `fallback` —— 找不到分區時怎麼辦
///
/// 回傳的順序是穩定的（依分區的 sort_order，再依 priority），
/// 因為快照測試與稽核都需要可重現。
pub fn plan_jobs<'a>(
    lines: &[RoutableLine],
    stations: &'a [Station],
    bindings: &[Binding],
    reason: TicketReason,
    fallback: Fallback<'a>,
) -> Vec<PlannedJob> {
    let station_by_id: BTreeMap<&str, &Station> =
        stations.iter().map(|s| (s.id.as_str(), s)).collect();

    // 依分區分組。用 BTreeMap 而不是 HashMap：輸出順序必須可重現。
    let mut grouped: BTreeMap<Option<&str>, Vec<&RoutableLine>> = BTreeMap::new();
    for line in lines {
        // 分區存在但已被停用／刪除時，當作沒設 —— 掉到 fallback 比印到
        // 一台已經拔掉的機器好。
        let station = line
            .resolve_station()
            .filter(|id| station_by_id.contains_key(id));
        grouped.entry(station).or_default().push(line);
    }

    let mut jobs = Vec::new();
    let mut sorted: Vec<_> = grouped.into_iter().collect();
    sorted.sort_by_key(|(station, _)| {
        station
            .and_then(|id| station_by_id.get(id))
            .map(|s| (0, s.sort_order, s.id.clone()))
            // 沒有分區的排最後：它是例外，不該擠在正常的單中間。
            .unwrap_or((1, 0, String::new()))
    });

    for (station_id, group) in sorted {
        let line_ids: Vec<String> = group.iter().map(|l| l.line_id.clone()).collect();

        let Some(station_id) = station_id else {
            if let Fallback::Printer(printer_id) = fallback {
                jobs.push(PlannedJob {
                    printer_id: printer_id.to_string(),
                    station_id: None,
                    station_name: None,
                    line_ids,
                    reason,
                    // 沒設分區的品項印到櫃檯，對店員來說就是「代印」。
                    is_backup: true,
                });
            }
            continue;
        };
        let station = station_by_id[station_id];

        let mut targets: Vec<&Binding> = bindings
            .iter()
            .filter(|b| b.station_id == station_id)
            .collect();
        targets.sort_by(|a, b| {
            a.priority
                .cmp(&b.priority)
                .then(a.printer_id.cmp(&b.printer_id))
        });

        // failover 只取第一台；always 的每一台都要印。
        // 兩種同時存在時：優先序最前的 failover 一台 + 全部的 always。
        let first_failover = targets
            .iter()
            .find(|b| b.mode == BindingMode::Failover)
            .map(|b| b.printer_id.as_str());
        let chosen: Vec<&Binding> = targets
            .into_iter()
            .filter(|b| {
                b.mode == BindingMode::Always || Some(b.printer_id.as_str()) == first_failover
            })
            .collect();

        if chosen.is_empty() {
            // 分區存在但沒綁任何機器。**不要安靜跳過** ——
            // 落到 fallback 至少會印出來，店員看到「代印」就知道設定漏了。
            if let Fallback::Printer(printer_id) = fallback {
                jobs.push(PlannedJob {
                    printer_id: printer_id.to_string(),
                    station_id: Some(station.id.clone()),
                    station_name: Some(station.name.clone()),
                    line_ids,
                    reason,
                    is_backup: true,
                });
            }
            continue;
        }

        for binding in chosen {
            jobs.push(PlannedJob {
                printer_id: binding.printer_id.clone(),
                station_id: Some(station.id.clone()),
                station_name: Some(station.name.clone()),
                line_ids: line_ids.clone(),
                reason,
                is_backup: false,
            });
        }
    }

    // split_per_item：一行一張。放在最後統一展開，前面的路由規則才不必
    // 每一條都處理「一張還是很多張」。
    jobs.into_iter()
        .flat_map(|job| {
            let split = job
                .station_id
                .as_deref()
                .and_then(|id| station_by_id.get(id))
                .map(|s| s.split_per_item)
                .unwrap_or(false);
            if !split || job.line_ids.len() <= 1 {
                return vec![job];
            }
            job.line_ids
                .iter()
                .map(|id| PlannedJob {
                    line_ids: vec![id.clone()],
                    ..job.clone()
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn station(id: &str, name: &str, order: i64) -> Station {
        Station {
            id: id.into(),
            name: name.into(),
            template: "kitchen".into(),
            split_per_item: false,
            sort_order: order,
        }
    }

    fn bind(station: &str, printer: &str, priority: i64, mode: BindingMode) -> Binding {
        Binding {
            station_id: station.into(),
            printer_id: printer.into(),
            priority,
            mode,
        }
    }

    fn line(id: &str, item_station: Option<&str>, cat_station: Option<&str>) -> RoutableLine {
        RoutableLine {
            line_id: id.into(),
            station_id: item_station.map(String::from),
            category_station_id: cat_station.map(String::from),
        }
    }

    fn stations() -> Vec<Station> {
        vec![station("drink", "飲料吧", 10), station("hot", "熱炒區", 20)]
    }

    #[test]
    fn each_station_gets_only_its_own_lines() {
        let jobs = plan_jobs(
            &[
                line("l1", Some("drink"), None),
                line("l2", Some("hot"), None),
                line("l3", None, Some("drink")),
            ],
            &stations(),
            &[
                bind("drink", "p-drink", 0, BindingMode::Failover),
                bind("hot", "p-hot", 0, BindingMode::Failover),
            ],
            TicketReason::NewOrder,
            Fallback::Printer("p-counter"),
        );

        assert_eq!(jobs.len(), 2);
        // sort_order 決定順序：飲料吧(10) 在熱炒區(20) 前面。
        assert_eq!(jobs[0].printer_id, "p-drink");
        assert_eq!(jobs[0].line_ids, ["l1", "l3"]);
        assert_eq!(jobs[1].printer_id, "p-hot");
        assert_eq!(jobs[1].line_ids, ["l2"]);
    }

    #[test]
    fn the_item_overrides_its_category() {
        // 「這一款熱飲要走熱炒區」是店長設定得出來的事。
        let jobs = plan_jobs(
            &[line("l1", Some("hot"), Some("drink"))],
            &stations(),
            &[
                bind("drink", "p-drink", 0, BindingMode::Failover),
                bind("hot", "p-hot", 0, BindingMode::Failover),
            ],
            TicketReason::NewOrder,
            Fallback::Printer("p-counter"),
        );
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].printer_id, "p-hot");
    }

    #[test]
    fn failover_prints_once_and_always_prints_every_time() {
        let jobs = plan_jobs(
            &[line("l1", Some("hot"), None)],
            &stations(),
            &[
                bind("hot", "p-hot", 0, BindingMode::Failover),
                // 備援機：只有第一台掛掉時才輪到它，所以現在不該出現。
                bind("hot", "p-backup", 1, BindingMode::Failover),
                // 櫃檯留底：每一次都要印。
                bind("hot", "p-counter", 2, BindingMode::Always),
            ],
            TicketReason::NewOrder,
            Fallback::Printer("p-counter"),
        );
        let printers: Vec<&str> = jobs.iter().map(|j| j.printer_id.as_str()).collect();
        assert_eq!(printers, ["p-hot", "p-counter"]);
    }

    #[test]
    fn an_unrouted_item_still_gets_printed_somewhere() {
        // ★ 「系統知道有東西沒印但沒告訴任何人」是 POS 最常見客訴的技術根因。
        // 沒設分區時寧可印到櫃檯，而且要標成代印讓店員看得出設定漏了。
        let jobs = plan_jobs(
            &[line("l1", None, None)],
            &stations(),
            &[],
            TicketReason::NewOrder,
            Fallback::Printer("p-counter"),
        );
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].printer_id, "p-counter");
        assert!(
            jobs[0].is_backup,
            "代印必須標記，不然沒人知道那張單要拿去哪"
        );
    }

    #[test]
    fn a_station_with_no_printer_falls_back_instead_of_vanishing() {
        let jobs = plan_jobs(
            &[line("l1", Some("hot"), None)],
            &stations(),
            &[], // 分區存在，但沒綁任何機器
            TicketReason::NewOrder,
            Fallback::Printer("p-counter"),
        );
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].printer_id, "p-counter");
        assert_eq!(jobs[0].station_name.as_deref(), Some("熱炒區"));
        assert!(jobs[0].is_backup);
    }

    #[test]
    fn a_deleted_station_does_not_send_paper_to_a_machine_that_is_gone() {
        // 品項還指著一個已經刪掉的分區。掉到 fallback 比印到拔掉的機器好。
        let jobs = plan_jobs(
            &[line("l1", Some("ghost"), None)],
            &stations(),
            &[bind("ghost", "p-ghost", 0, BindingMode::Failover)],
            TicketReason::NewOrder,
            Fallback::Printer("p-counter"),
        );
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].printer_id, "p-counter");
    }

    #[test]
    fn split_per_item_makes_one_ticket_per_line() {
        // 燒烤與貼杯標籤：一張單跟著一份餐走。
        let mut ss = stations();
        ss[0].split_per_item = true;
        let jobs = plan_jobs(
            &[
                line("l1", Some("drink"), None),
                line("l2", Some("drink"), None),
                line("l3", Some("hot"), None),
            ],
            &ss,
            &[
                bind("drink", "p-drink", 0, BindingMode::Failover),
                bind("hot", "p-hot", 0, BindingMode::Failover),
            ],
            TicketReason::NewOrder,
            Fallback::Printer("p-counter"),
        );
        assert_eq!(jobs.len(), 3);
        assert_eq!(jobs[0].line_ids, ["l1"]);
        assert_eq!(jobs[1].line_ids, ["l2"]);
        assert_eq!(jobs[2].line_ids, ["l3"]);
    }

    #[test]
    fn dropping_is_possible_but_only_when_asked_for() {
        let jobs = plan_jobs(
            &[line("l1", None, None)],
            &stations(),
            &[],
            TicketReason::NewOrder,
            Fallback::Drop,
        );
        assert!(jobs.is_empty());
    }

    #[test]
    fn the_reason_travels_with_every_ticket() {
        // 加點與退點在廚房是完全不同的動作。混在一起印會讓廚師重做整桌。
        for reason in [
            TicketReason::NewOrder,
            TicketReason::AddItems,
            TicketReason::Void,
            TicketReason::Reprint,
        ] {
            let jobs = plan_jobs(
                &[line("l1", Some("hot"), None)],
                &stations(),
                &[bind("hot", "p-hot", 0, BindingMode::Failover)],
                reason,
                Fallback::Drop,
            );
            assert_eq!(jobs[0].reason, reason);
        }
    }

    #[test]
    fn planning_is_deterministic() {
        // 稽核與快照測試都要求可重現：同樣的輸入必須排出逐位元相同的結果。
        let lines = [
            line("l3", Some("hot"), None),
            line("l1", Some("drink"), None),
            line("l2", Some("hot"), None),
        ];
        let bindings = [
            bind("hot", "p-b", 1, BindingMode::Always),
            bind("hot", "p-a", 0, BindingMode::Always),
            bind("drink", "p-drink", 0, BindingMode::Failover),
        ];
        let first = plan_jobs(
            &lines,
            &stations(),
            &bindings,
            TicketReason::NewOrder,
            Fallback::Drop,
        );
        for _ in 0..20 {
            assert_eq!(
                plan_jobs(
                    &lines,
                    &stations(),
                    &bindings,
                    TicketReason::NewOrder,
                    Fallback::Drop
                ),
                first
            );
        }
        let printers: Vec<&str> = first.iter().map(|j| j.printer_id.as_str()).collect();
        assert_eq!(printers, ["p-drink", "p-a", "p-b"]);
    }
}
