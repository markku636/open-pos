//! 收據排版：字寬、斷行、分欄。
//!
//! 出單機是**等寬網格**裝置：58mm 一行 32 個半形位、80mm 一行 48 個。
//! 中文字佔兩個位。所有對齊都建立在「一個字佔幾格」這件事上，
//! 所以這個模組是收據正確性的地基。

use unicode_width::UnicodeWidthChar;

/// 一個字元佔幾格。
///
/// ★ 用 `width_cjk()`（Ambiguous = 2）而**不是** `width()`。
///
/// 「※」「℃」「①」「—」「’」在 UAX #11 裡是 Ambiguous，
/// 但在 CJK 語境下（包含所有 Big5 與中文點陣字型）它們就是全形。
/// 判成 1 會讓右欄整排往左偏一格 —— 而收據上「金額欄沒對齊」是一眼就看得出的瑕疵。
pub fn char_width(c: char) -> usize {
    // 控制字元不佔位（它們不該出現在收據上，但別讓它們把版面撐爆）。
    c.width_cjk().unwrap_or(0)
}

/// 一段文字佔幾格。
pub fn display_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

/// 截斷到指定格數（不補空白）。用於「這一欄就是放不下」的情況。
pub fn truncate(s: &str, width: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = char_width(c);
        if used + w > width {
            break;
        }
        out.push(c);
        used += w;
    }
    out
}

/// 補到指定格數。
pub fn pad(s: &str, width: usize, align: Align) -> String {
    let text = truncate(s, width);
    let gap = width.saturating_sub(display_width(&text));
    match align {
        Align::Left => format!("{text}{}", " ".repeat(gap)),
        Align::Right => format!("{}{text}", " ".repeat(gap)),
        Align::Center => {
            let left = gap / 2;
            format!("{}{text}{}", " ".repeat(left), " ".repeat(gap - left))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// 斷行。
///
/// 規則：
/// * 中文可以在任何字之後折 —— 中文沒有詞界，硬折是正常的。
/// * 英數字盡量整個單字帶下去；但單字本身超過欄寬就硬折
///   （否則會無限迴圈，而且一個超長的 SKU 會把整張單推爆）。
/// * 明確的換行符號一律尊重。
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let mut out = Vec::new();
    for para in text.split('\n') {
        out.extend(wrap_paragraph(para, width));
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn wrap_paragraph(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0usize;
    // 待決的英數字單字：還不確定要不要帶到下一行。
    let mut word = String::new();
    let mut word_w = 0usize;

    let flush_word =
        |cur: &mut String, cur_w: &mut usize, word: &mut String, word_w: &mut usize| {
            cur.push_str(word);
            *cur_w += *word_w;
            word.clear();
            *word_w = 0;
        };

    for c in text.chars() {
        let w = char_width(c);
        let is_word_char = c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_';

        if is_word_char {
            // 單字本身就超過整行 → 硬折，不然永遠放不下。
            if word_w + w > width {
                flush_word(&mut cur, &mut cur_w, &mut word, &mut word_w);
                lines.push(std::mem::take(&mut cur));
                cur_w = 0;
            }
            word.push(c);
            word_w += w;
            continue;
        }

        // 遇到非單字字元：先把待決單字放進目前這行。
        if word_w > 0 {
            if cur_w + word_w > width && cur_w > 0 {
                lines.push(std::mem::take(&mut cur));
                cur_w = 0;
            }
            flush_word(&mut cur, &mut cur_w, &mut word, &mut word_w);
        }

        // 只吃「折行之後」的行首空白 —— 那是斷行造成的，不是作者要的。
        // **刻意的縮排必須保留**：廚房單的加購項是用縮排表示層級的，
        // 吃掉之後就看不出哪個選項屬於哪一道菜。
        if c == ' ' && cur_w == 0 && !lines.is_empty() {
            continue;
        }
        if cur_w + w > width {
            lines.push(std::mem::take(&mut cur));
            cur_w = 0;
            if c == ' ' {
                continue;
            }
        }
        cur.push(c);
        cur_w += w;
    }

    if word_w > 0 {
        if cur_w + word_w > width && cur_w > 0 {
            lines.push(std::mem::take(&mut cur));
            cur_w = 0;
        }
        flush_word(&mut cur, &mut cur_w, &mut word, &mut word_w);
    }
    if !cur.is_empty() || lines.is_empty() {
        lines.push(cur);
    }
    // 行尾的空白對出單機沒有意義，卻會讓快照 diff 很吵、也會讓
    // 「這一行有多寬」的斷言看起來莫名其妙。
    for l in &mut lines {
        while l.ends_with(' ') {
            l.pop();
        }
    }
    lines
}

/// 依權重把一行切成幾欄。
///
/// 規則：**第一欄以外的每一欄先取「比例的無條件進位」，剩下的全給第一欄。**
///
/// 例：`split_columns(48, &[3, 1, 1])` → `[28, 10, 10]`
///     `split_columns(32, &[3, 1, 1])` → `[18, 7, 7]`
///
/// 為什麼不是單純按比例四捨五入：收據的第一欄是品名，它**會斷行**；
/// 數量與金額欄不會，被擠掉一格就直接截斷或對不齊。
/// 所以讓數字欄拿滿它該有的寬度，把零頭全部丟給唯一能優雅吸收它的品名欄。
///
/// 同一份 ReceiptDoc 在 58mm 與 80mm 下走同一段程式碼，差別只在 `cols`。
pub fn split_columns(cols: usize, weights: &[u8]) -> Vec<usize> {
    if weights.is_empty() {
        return Vec::new();
    }
    if weights.len() == 1 {
        return vec![cols];
    }
    let total: usize = weights.iter().map(|w| *w as usize).sum();
    if total == 0 {
        let each = cols / weights.len();
        let mut out = vec![each; weights.len()];
        out[0] += cols - each * weights.len();
        return out;
    }

    let mut out = vec![0usize; weights.len()];
    let mut rest = 0usize;
    for i in 1..weights.len() {
        // 無條件進位：num.div_ceil(den)
        let w = (cols * weights[i] as usize).div_ceil(total);
        out[i] = w;
        rest += w;
    }
    out[0] = cols.saturating_sub(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cjk_characters_take_two_cells() {
        assert_eq!(display_width("珍珠奶茶"), 8);
        assert_eq!(display_width("abc"), 3);
        assert_eq!(display_width("珍奶x2"), 6);
    }

    #[test]
    fn ambiguous_width_characters_are_treated_as_full_width() {
        // 這一條是「右欄對不齊」的根因。UAX #11 說它們 Ambiguous，
        // 但在中文收據上它們就是全形。
        for c in ['※', '℃', '—', '①'] {
            assert_eq!(char_width(c), 2, "{c} 在 CJK 語境應佔兩格");
        }
    }

    #[test]
    fn pad_aligns_within_the_grid() {
        assert_eq!(pad("珍奶", 8, Align::Left), "珍奶    ");
        assert_eq!(pad("珍奶", 8, Align::Right), "    珍奶");
        assert_eq!(pad("60", 6, Align::Right), "    60");
        // 放不下就截斷，不要讓它把後面的欄推歪。
        assert_eq!(pad("珍珠奶茶特大杯", 6, Align::Left), "珍珠奶");
    }

    #[test]
    fn truncate_never_splits_a_wide_char_in_half() {
        // 截在半個中文字上會產生壞掉的位元組，出單機印出亂碼。
        assert_eq!(truncate("珍珠奶茶", 5), "珍珠");
        assert_eq!(truncate("珍珠奶茶", 6), "珍珠奶");
    }

    #[test]
    fn wrap_breaks_chinese_anywhere() {
        let lines = wrap("珍珠奶茶特大杯半糖去冰加珍珠", 12);
        assert_eq!(lines, vec!["珍珠奶茶特大", "杯半糖去冰加", "珍珠"]);
    }

    #[test]
    fn wrap_keeps_ascii_words_together() {
        let lines = wrap("order ABC12345 ready", 12);
        assert_eq!(lines, vec!["order", "ABC12345", "ready"]);
    }

    #[test]
    fn wrap_hard_breaks_a_word_longer_than_the_line() {
        // 一個超長的 SKU 不該把整張單推爆，也不該讓迴圈卡住。
        let lines = wrap("ABCDEFGHIJKLMNOPQRSTUVWXYZ", 10);
        assert_eq!(lines, vec!["ABCDEFGHIJ", "KLMNOPQRST", "UVWXYZ"]);
    }

    #[test]
    fn wrap_keeps_deliberate_indentation_on_the_first_line() {
        // 廚房單用縮排表示「這個選項屬於上面那道菜」。吃掉縮排就看不出層級了。
        assert_eq!(wrap("   - 去冰", 20), vec!["   - 去冰"]);
        // 但折行造成的行首空白要吃掉 —— 那不是作者要的。
        assert_eq!(wrap("aaa bbb ccc", 4), vec!["aaa", "bbb", "ccc"]);
    }

    #[test]
    fn wrap_respects_explicit_newlines() {
        assert_eq!(wrap("甲\n乙", 10), vec!["甲", "乙"]);
    }

    #[test]
    fn wrap_of_empty_text_is_one_empty_line() {
        assert_eq!(wrap("", 10), vec![""]);
    }

    #[test]
    fn columns_fill_the_paper_exactly() {
        assert_eq!(split_columns(48, &[3, 1, 1]), vec![28, 10, 10]);
        assert_eq!(split_columns(32, &[3, 1, 1]), vec![18, 7, 7]);
        // 加總必須剛好等於紙寬，否則右邊會多出一格或被吃掉一格。
        for cols in [32usize, 48] {
            for w in [vec![3u8, 1, 1], vec![1, 1], vec![5, 2, 3], vec![1]] {
                assert_eq!(split_columns(cols, &w).iter().sum::<usize>(), cols);
            }
        }
    }
}
