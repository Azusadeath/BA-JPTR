//! Local changes are rebased onto current computed text, never onto a saved final result.
use crate::{Result, invalid};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

pub fn difference(before: &str, after: &str) -> Vec<Change> {
    let a: Vec<_> = before.chars().collect();
    let b: Vec<_> = after.chars().collect();
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let a_end = a.len() - suffix;
    let b_end = b.len() - suffix;
    if prefix == a_end && prefix == b_end {
        return vec![];
    }
    let n = a_end - prefix;
    let m = b_end - prefix;
    // Bound memory/time for unusually long edits. A coarser change is safe: it
    // may report a conflict, but never silently freezes supposedly unchanged text.
    if n.saturating_add(1).saturating_mul(m.saturating_add(1)) > 1_000_000 {
        return vec![Change {
            start: prefix,
            end: a_end,
            text: b[prefix..b_end].iter().collect(),
        }];
    }
    let stride = m + 1;
    let mut lengths = vec![0u32; (n + 1) * stride];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lengths[i * stride + j] = if a[prefix + i] == b[prefix + j] {
                1 + lengths[(i + 1) * stride + j + 1]
            } else {
                lengths[(i + 1) * stride + j].max(lengths[i * stride + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut changes = Vec::new();
    while i < n || j < m {
        if i < n && j < m && a[prefix + i] == b[prefix + j] {
            i += 1;
            j += 1;
            continue;
        }
        let start = i;
        let mut text = String::new();
        while i < n || j < m {
            if i < n && j < m && a[prefix + i] == b[prefix + j] {
                break;
            }
            if j < m && (i == n || lengths[i * stride + j + 1] > lengths[(i + 1) * stride + j]) {
                text.push(b[prefix + j]);
                j += 1;
            } else {
                i += 1;
            }
        }
        changes.push(Change {
            start: prefix + start,
            end: prefix + i,
            text,
        });
    }
    changes
}

// Identical repeated characters can admit several equally valid edit locations.
// Reserve every equivalent location rather than guessing which one the user meant.
fn footprint(base: &[char], change: &Change) -> (usize, usize) {
    let text: Vec<_> = change.text.chars().collect();
    let (mut left, mut right) = (change.start, change.start);
    let length = change.end - change.start;
    let mut budget = 1_000_000usize;
    while left > 0 {
        if budget < text.len() + 1 {
            return (0, base.len());
        }
        budget -= text.len() + 1;
        if !std::iter::once(&base[left - 1])
            .chain(text.iter())
            .eq(text.iter().chain(std::iter::once(&base[left + length - 1])))
        {
            break;
        }
        left -= 1;
    }
    while right + length < base.len() {
        if budget < text.len() + 1 {
            return (0, base.len());
        }
        budget -= text.len() + 1;
        if !text
            .iter()
            .chain(std::iter::once(&base[right + length]))
            .eq(std::iter::once(&base[right]).chain(text.iter()))
        {
            break;
        }
        right += 1;
    }
    (left, right + length)
}

pub fn apply(base: &str, changes: &[Change], current: &str) -> Result<String> {
    let external = difference(base, current);
    let current_chars: Vec<_> = current.chars().collect();
    let base_chars: Vec<_> = base.chars().collect();
    let base_len = base_chars.len();
    let mut result = String::new();
    let mut cursor = 0;
    for change in changes {
        if change.start > change.end || change.end > base_len {
            return Err(invalid("局部修改的位置无效。"));
        }
        let mut shift = 0isize;
        for other in &external {
            let (start, end) = footprint(&base_chars, change);
            let (other_start, other_end) = footprint(&base_chars, other);
            let overlap =
                if change.start == change.end && (change.start == 0 || change.start == base_len) {
                    other.start == other.end && other.start == change.start
                } else if start == end {
                    (other_start == other_end && other_start == start)
                        || (other_start <= start
                            && start < other_end
                            && change.start != 0
                            && change.start != base_len)
                } else if other_start == other_end {
                    start <= other_start && other_start < end
                } else {
                    start < other_end && other_start < end
                };
            if overlap {
                return Err(invalid(
                    "局部修改与当前词表或前序修改冲突，请打开该项重新编辑，或明确选择整段覆盖。",
                ));
            }
            if other.end <= change.start {
                shift += other.text.chars().count() as isize - (other.end - other.start) as isize;
            }
        }
        let start = change
            .start
            .checked_add_signed(shift)
            .ok_or_else(|| invalid("局部修改的位置无效。"))?;
        let end = change
            .end
            .checked_add_signed(shift)
            .ok_or_else(|| invalid("局部修改的位置无效。"))?;
        if start < cursor || end > current_chars.len() {
            return Err(invalid("局部修改的位置无效。"));
        }
        result.extend(&current_chars[cursor..start]);
        result.push_str(&change.text);
        cursor = end;
    }
    result.extend(&current_chars[cursor..]);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_changes_reconstruct_small_inputs_and_bound_large_edits() {
        let mut values = vec![String::new()];
        for _ in 0..3 {
            let previous = values.clone();
            for value in previous {
                for c in ['甲', 'a', '\n'] {
                    values.push(format!("{value}{c}"));
                }
            }
        }
        values.sort();
        values.dedup();
        for before in &values {
            for after in &values {
                assert_eq!(
                    apply(before, &difference(before, after), before).unwrap(),
                    *after
                );
            }
        }
        let before = "甲".repeat(2000);
        let after = "乙".repeat(2000);
        let patch = difference(&before, &after);
        assert_eq!(patch.len(), 1);
        assert_eq!(apply(&before, &patch, &before).unwrap(), after);
        assert!(apply(&before, &patch, &"丙".repeat(2000)).is_err());
    }
    #[test]
    fn separated_changes_keep_current_unedited_words_and_unicode_newlines() {
        let base = "甲，旧名字，乙\n尾";
        let changed = "甲！旧名字，乙\n尾 ";
        let patch = difference(base, changed);
        assert_eq!(
            apply(base, &patch, "甲，新名字，乙\n尾").unwrap(),
            "甲！新名字，乙\n尾 "
        );
        assert_eq!(apply(base, &patch, base).unwrap(), changed);
    }
    #[test]
    fn overlapping_and_competing_insertions_are_conflicts() {
        assert!(apply("名字", &difference("名字", "新名字"), "另一名字").is_err());
        assert!(apply("甲乙", &difference("甲乙", "甲！乙"), "甲？乙").is_err());
        assert!(apply("甲哈哈乙", &difference("甲哈哈乙", "甲哈乙"), "甲呵哈乙").is_err());
    }
}
