/// Estimated word-level timestamp from TTS duration.
#[derive(Debug, Clone)]
pub struct WordTimestamp {
    pub word: String,
    pub start_secs: f64,
    pub end_secs: f64,
}

/// Estimate word-level timestamps from text and total audio duration.
/// Distributes time proportionally based on character count per word.
pub fn estimate_word_timestamps(text: &str, total_duration: f64) -> Vec<WordTimestamp> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() || total_duration <= 0.0 {
        return Vec::new();
    }

    let total_chars: usize = words.iter().map(|w| w.len()).sum();
    if total_chars == 0 {
        return Vec::new();
    }

    let gap = 0.05_f64; // small gap between words
    let total_gap = gap * (words.len().saturating_sub(1)) as f64;
    // If gaps would consume more than half the duration, reduce them
    let effective_gap = if total_gap > total_duration * 0.5 {
        (total_duration * 0.5) / (words.len().saturating_sub(1)).max(1) as f64
    } else {
        gap
    };
    let available_duration = total_duration - effective_gap * words.len().saturating_sub(1) as f64;

    let mut timestamps = Vec::with_capacity(words.len());
    let mut cursor = 0.0_f64;

    for (i, word) in words.iter().enumerate() {
        let proportion = word.len() as f64 / total_chars as f64;
        let word_duration = proportion * available_duration;
        let start = cursor;
        let end = if i == words.len() - 1 {
            total_duration // last word ends exactly at total_duration
        } else {
            cursor + word_duration
        };

        timestamps.push(WordTimestamp {
            word: word.to_string(),
            start_secs: start,
            end_secs: end,
        });

        cursor = end + effective_gap;
    }

    timestamps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_basic() {
        let words = estimate_word_timestamps("The quick brown fox jumps", 5.0);
        assert_eq!(words.len(), 5);
        assert!(words[0].start_secs < 0.001);
        assert!((words.last().unwrap().end_secs - 5.0).abs() < 0.001);
        // All timestamps should be monotonically increasing
        for i in 1..words.len() {
            assert!(words[i].start_secs >= words[i - 1].end_secs);
        }
    }

    #[test]
    fn test_estimate_single_word() {
        let words = estimate_word_timestamps("Hello", 3.0);
        assert_eq!(words.len(), 1);
        assert!(words[0].start_secs.abs() < 0.001);
        assert!((words[0].end_secs - 3.0).abs() < 0.001);
        assert_eq!(words[0].word, "Hello");
    }

    #[test]
    fn test_estimate_empty_text() {
        let words = estimate_word_timestamps("", 5.0);
        assert!(words.is_empty());

        let words = estimate_word_timestamps("   ", 5.0);
        assert!(words.is_empty());
    }

    #[test]
    fn test_estimate_proportional() {
        let words = estimate_word_timestamps("I extraordinary", 10.0);
        assert_eq!(words.len(), 2);
        // "extraordinary" (13 chars) should get much more time than "I" (1 char)
        let dur_i = words[0].end_secs - words[0].start_secs;
        let dur_extra = words[1].end_secs - words[1].start_secs;
        assert!(dur_extra > dur_i * 5.0);
    }

    #[test]
    fn test_estimate_respects_duration() {
        let words = estimate_word_timestamps("one two three four five six seven", 2.5);
        assert_eq!(words.len(), 7);
        assert!((words.last().unwrap().end_secs - 2.5).abs() < 0.001);
    }

    #[test]
    fn test_estimate_zero_duration() {
        let words = estimate_word_timestamps("Hello world", 0.0);
        assert!(words.is_empty());
    }
}
