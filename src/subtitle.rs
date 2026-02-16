use crate::tts::timestamps::WordTimestamp;

/// A single subtitle entry (one or more words shown together).
#[derive(Debug, Clone)]
pub struct SubtitleEntry {
    pub index: usize,
    pub start_secs: f64,
    pub end_secs: f64,
    pub text: String,
}

/// Group word timestamps into subtitle entries.
/// Groups up to `max_words_per_line` words per subtitle entry for readability.
pub fn group_into_subtitles(
    words: &[WordTimestamp],
    max_words_per_line: usize,
) -> Vec<SubtitleEntry> {
    if words.is_empty() {
        return Vec::new();
    }
    let max = max_words_per_line.max(1);
    let mut entries = Vec::new();
    let mut i = 0;
    let mut index = 1;

    while i < words.len() {
        let end = (i + max).min(words.len());
        let chunk = &words[i..end];
        let text = chunk
            .iter()
            .map(|w| w.word.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        entries.push(SubtitleEntry {
            index,
            start_secs: chunk[0].start_secs,
            end_secs: chunk.last().unwrap().end_secs,
            text,
        });
        index += 1;
        i = end;
    }

    entries
}

/// Write subtitle entries as SRT format string.
pub fn to_srt(entries: &[SubtitleEntry]) -> String {
    let mut out = String::new();
    for entry in entries {
        out.push_str(&format!("{}\n", entry.index));
        out.push_str(&format!(
            "{} --> {}\n",
            format_srt_time(entry.start_secs),
            format_srt_time(entry.end_secs),
        ));
        out.push_str(&entry.text);
        out.push_str("\n\n");
    }
    out
}

/// Format seconds as SRT timestamp: "HH:MM:SS,mmm"
fn format_srt_time(secs: f64) -> String {
    let total_ms = (secs * 1000.0).round() as u64;
    let ms = total_ms % 1000;
    let total_s = total_ms / 1000;
    let s = total_s % 60;
    let total_m = total_s / 60;
    let m = total_m % 60;
    let h = total_m / 60;
    format!("{h:02}:{m:02}:{s:02},{ms:03}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_srt_time() {
        assert_eq!(format_srt_time(0.0), "00:00:00,000");
        assert_eq!(format_srt_time(65.5), "00:01:05,500");
        assert_eq!(format_srt_time(3661.123), "01:01:01,123");
        assert_eq!(format_srt_time(0.999), "00:00:00,999");
    }

    #[test]
    fn test_group_into_subtitles() {
        let words: Vec<WordTimestamp> = (0..20)
            .map(|i| WordTimestamp {
                word: format!("word{i}"),
                start_secs: i as f64 * 0.5,
                end_secs: (i as f64 + 1.0) * 0.5,
            })
            .collect();
        let entries = group_into_subtitles(&words, 6);
        assert_eq!(entries.len(), 4); // 6+6+6+2
        assert_eq!(entries[0].index, 1);
        assert_eq!(entries[0].text, "word0 word1 word2 word3 word4 word5");
        assert_eq!(entries[3].text, "word18 word19");
    }

    #[test]
    fn test_to_srt_format() {
        let entries = vec![
            SubtitleEntry {
                index: 1,
                start_secs: 0.0,
                end_secs: 2.5,
                text: "Hello world".into(),
            },
            SubtitleEntry {
                index: 2,
                start_secs: 2.5,
                end_secs: 5.0,
                text: "Goodbye world".into(),
            },
        ];
        let srt = to_srt(&entries);
        assert!(srt.contains("1\n00:00:00,000 --> 00:00:02,500\nHello world\n"));
        assert!(srt.contains("2\n00:00:02,500 --> 00:00:05,000\nGoodbye world\n"));
    }

    #[test]
    fn test_group_empty() {
        let entries = group_into_subtitles(&[], 6);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_group_fewer_than_max() {
        let words = vec![
            WordTimestamp {
                word: "one".into(),
                start_secs: 0.0,
                end_secs: 1.0,
            },
            WordTimestamp {
                word: "two".into(),
                start_secs: 1.0,
                end_secs: 2.0,
            },
        ];
        let entries = group_into_subtitles(&words, 6);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "one two");
    }
}
