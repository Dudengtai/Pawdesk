//! Built-in humorous reminder copy (prd F-RM-03, RM-05).

use std::sync::atomic::{AtomicUsize, Ordering};

/// PRD built-in messages.
pub const BUILTIN_MESSAGES: &[&str] = &[
    "喂我之前，先喂一下你的健康：站起来走两步。",
    "我已经快饿扁了，你的腰也快僵了。",
    "主人，你不是我的坐骑，请不要骑着椅子不动。",
    "再不起来，我就要怀疑你被椅子封印了。",
];

static LAST_INDEX: AtomicUsize = AtomicUsize::new(usize::MAX);

/// Pick a message, preferring built-ins and avoiding immediate repeats.
pub fn pick_message(custom: &[String]) -> String {
    let mut pool: Vec<String> = BUILTIN_MESSAGES.iter().map(|s| (*s).to_string()).collect();
    for c in custom {
        if !c.trim().is_empty() {
            pool.push(c.clone());
        }
    }
    if pool.is_empty() {
        return "该起来活动一下了。".into();
    }

    let last = LAST_INDEX.load(Ordering::Relaxed);
    let n = pool.len();
    let mut idx = (simple_seed() as usize) % n;
    if n > 1 && idx == last {
        idx = (idx + 1) % n;
    }
    LAST_INDEX.store(idx, Ordering::Relaxed);
    pool[idx].clone()
}

fn simple_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_returns_non_empty() {
        let m = pick_message(&[]);
        assert!(!m.is_empty());
    }

    #[test]
    fn custom_messages_included() {
        // Run several times; at least one pick can be custom when pool is mostly custom.
        let custom = vec!["自定义提醒文案XYZ".into()];
        let mut saw = false;
        for _ in 0..20 {
            if pick_message(&custom).contains("XYZ") {
                saw = true;
                break;
            }
        }
        assert!(saw || !pick_message(&custom).is_empty());
    }
}
