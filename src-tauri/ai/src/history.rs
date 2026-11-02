//! The document's version history as the agent sees it: a list of saved versions
//! (numbered v1, v2… exactly as the history rail shows them), any version's text, and
//! paragraph-level diffs between versions or against the current text.

use similar::{ChangeTag, TextDiff};

/// One saved version, oldest first. `n` is the number the author sees (v1 is the first).
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub n: usize,
    /// "you" or "agent".
    pub author: String,
    pub label: String,
    /// Milliseconds since the Unix epoch.
    pub created_at: i64,
    pub content: String,
}

/// What the agent compares against: a saved version, or the text as it is now.
pub enum Side<'a> {
    Version(&'a Snapshot),
    Current(&'a str),
}

const LIST_MAX: usize = 60;
const TEXT_MAX: usize = 24_000;
const DIFF_MAX: usize = 16_000;

fn words(s: &str) -> usize {
    s.split_whitespace().count()
}

/// Local wall-clock time, given the author's UTC offset in minutes.
pub fn local_time(ms: i64, offset_min: i32) -> String {
    let secs = ms.div_euclid(1000) + offset_min as i64 * 60;
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{d} {} {y}, {:02}:{:02}",
        MONTHS[(m - 1) as usize],
        tod / 3600,
        (tod % 3600) / 60
    )
}

fn ago(ms: i64, now: i64) -> String {
    let mins = (now - ms).max(0) / 60_000;
    match mins {
        0 => "just now".into(),
        1..=59 => format!("{mins} min ago"),
        60..=1439 => format!("{} h ago", mins / 60),
        _ => match mins / 1440 {
            1 => "yesterday".into(),
            d => format!("{d} days ago"),
        },
    }
}

/// Paragraph-level word churn between two texts: (added, removed).
fn churn(old: &str, new: &str) -> (usize, usize) {
    let (a, b) = (paragraphs(old), paragraphs(new));
    let diff = TextDiff::configure().diff_slices(&a, &b);
    let (mut add, mut del) = (0, 0);
    for op in diff.iter_all_changes() {
        match op.tag() {
            ChangeTag::Insert => add += words(op.value()),
            ChangeTag::Delete => del += words(op.value()),
            ChangeTag::Equal => {}
        }
    }
    (add, del)
}

fn paragraphs(s: &str) -> Vec<&str> {
    s.split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect()
}

/// Newest first, with when, who, what, and how much changed from the version before.
pub fn list(snaps: &[Snapshot], now_ms: i64, offset_min: i32) -> String {
    if snaps.is_empty() {
        return "This document has no saved versions yet.".into();
    }
    let mut out = format!(
        "{} saved versions, newest first (v{} is what was last saved):\n",
        snaps.len(),
        snaps.len()
    );
    for (i, s) in snaps.iter().enumerate().rev().take(LIST_MAX) {
        let who = if s.author == "agent" {
            "Forge"
        } else {
            "author"
        };
        let change = match i.checked_sub(1).map(|p| &snaps[p]) {
            Some(prev) => {
                let (add, del) = churn(&prev.content, &s.content);
                format!("+{add} / -{del} words")
            }
            None => "first version".into(),
        };
        out.push_str(&format!(
            "- v{} · {} ({}) · {} · \"{}\" · {} words · {}\n",
            s.n,
            local_time(s.created_at, offset_min),
            ago(s.created_at, now_ms),
            who,
            s.label,
            words(&s.content),
            change
        ));
    }
    if snaps.len() > LIST_MAX {
        out.push_str(&format!(
            "({} older versions not shown; ask for any by number.)\n",
            snaps.len() - LIST_MAX
        ));
    }
    out
}

pub fn text(s: &Snapshot) -> String {
    let body = crate::search::clip(&s.content, TEXT_MAX);
    format!("v{} (\"{}\"), as Markdown:\n\n{body}", s.n, s.label)
}

fn name(side: &Side) -> String {
    match side {
        Side::Version(s) => format!("v{}", s.n),
        Side::Current(_) => "the current text".into(),
    }
}

fn body<'a>(side: &Side<'a>) -> &'a str {
    match side {
        Side::Version(s) => &s.content,
        Side::Current(t) => t,
    }
}

/// Paragraph diff from `old` to `new`: removed paragraphs as "- ", added as "+ ",
/// runs of unchanged ones collapsed to a count (one neighbour kept for context).
pub fn diff(old: &Side, new: &Side) -> String {
    let (a, b) = (paragraphs(body(old)), paragraphs(body(new)));
    let d = TextDiff::configure().diff_slices(&a, &b);
    let changes: Vec<_> = d.iter_all_changes().collect();
    if changes.iter().all(|c| c.tag() == ChangeTag::Equal) {
        return format!("No differences between {} and {}.", name(old), name(new));
    }
    let near_change = |i: usize| {
        let lo = i.saturating_sub(1);
        let hi = (i + 1).min(changes.len() - 1);
        (lo..=hi).any(|j| changes[j].tag() != ChangeTag::Equal)
    };
    let (add, del) = churn(body(old), body(new));
    let mut out = format!(
        "Changes from {} to {} (+{add} / -{del} words). \"- \" removed, \"+ \" added, \"  \" unchanged context:\n\n",
        name(old),
        name(new)
    );
    let mut skipped = 0;
    for (i, c) in changes.iter().enumerate() {
        if c.tag() == ChangeTag::Equal && !near_change(i) {
            skipped += 1;
            continue;
        }
        if skipped > 0 {
            out.push_str(&format!(
                "  [{skipped} unchanged paragraph{}]\n\n",
                if skipped == 1 { "" } else { "s" }
            ));
            skipped = 0;
        }
        let mark = match c.tag() {
            ChangeTag::Insert => "+ ",
            ChangeTag::Delete => "- ",
            ChangeTag::Equal => "  ",
        };
        out.push_str(mark);
        out.push_str(c.value());
        out.push_str("\n\n");
        if out.len() > DIFF_MAX {
            out.push_str("[Diff cut short; compare a narrower range of versions.]\n");
            return out;
        }
    }
    if skipped > 0 {
        out.push_str(&format!(
            "  [{skipped} unchanged paragraph{}]\n",
            if skipped == 1 { "" } else { "s" }
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(n: usize, content: &str) -> Snapshot {
        Snapshot {
            n,
            author: "you".into(),
            label: format!("Edit {n}"),
            created_at: 1_793_600_000_000 + n as i64 * 3_600_000,
            content: content.into(),
        }
    }

    #[test]
    fn formats_local_time() {
        // 2026-11-02 05:46:40 UTC, shown in UTC+5:30.
        assert_eq!(local_time(1_793_598_400_000, 330), "2 Nov 2026, 11:16");
        assert_eq!(local_time(0, 0), "1 Jan 1970, 00:00");
    }

    #[test]
    fn lists_newest_first_with_churn() {
        let s = vec![
            snap(1, "One two.\n\nThree."),
            snap(2, "One two.\n\nThree four five."),
        ];
        let l = list(&s, s[1].created_at + 120_000, 0);
        let v2 = l.find("- v2").unwrap();
        let v1 = l.find("- v1").unwrap();
        assert!(v2 < v1, "{l}");
        assert!(l.contains("+3 / -1 words"), "{l}");
        assert!(l.contains("2 min ago"), "{l}");
        assert!(l.contains("first version"));
    }

    #[test]
    fn diffs_paragraphs_with_context() {
        let old = snap(1, "# Title\n\nKeep me.\n\nCut me.\n\nA\n\nB\n\nC\n\nD");
        let now = "# Title\n\nKeep me.\n\nA\n\nB\n\nC\n\nD\n\nNew ending.";
        let d = diff(&Side::Version(&old), &Side::Current(now));
        assert!(d.contains("- Cut me."), "{d}");
        assert!(d.contains("+ New ending."), "{d}");
        assert!(d.contains("unchanged paragraph"), "{d}");
        assert!(d.contains("from v1 to the current text"));
        assert_eq!(
            diff(&Side::Version(&old), &Side::Version(&old)),
            "No differences between v1 and v1."
        );
    }
}
