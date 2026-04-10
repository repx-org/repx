use serde_json::Value;

pub(crate) const DPI: &str = "300";
pub(crate) const FONT_NAME: &str = "Helvetica, Arial, sans-serif";
pub(crate) const RANK_SEP: &str = "0.6";
pub(crate) const NODE_SEP: &str = "0.4";
pub(crate) const GRAPH_PAD: &str = "0.5";
pub(crate) const JOB_FONT_SIZE: &str = "12";
pub(crate) const COLOR_CLUSTER_BORDER: &str = "#334155";
pub(crate) const COLOR_GROUP_BORDER: &str = "#1e40af";
pub(crate) const GROUP_FONT_SIZE: &str = "16";
pub(crate) const PARAM_SHAPE: &str = "note";
pub(crate) const PARAM_FILL: &str = "#FFFFFF";
pub(crate) const PARAM_BORDER: &str = "#94a3b8";
pub(crate) const PARAM_FONT_COLOR: &str = "#475569";
pub(crate) const PARAM_FONT_SIZE: &str = "9";
pub(crate) const PARAM_MAX_WIDTH: usize = 20;

pub(crate) const SG_CLUSTER_BORDER: &str = "#6366f1";
pub(crate) const SG_CLUSTER_BG: &str = "#EEF2FF";
pub(crate) const SG_SCATTER_FILL: &str = "#C7D2FE";
pub(crate) const SG_GATHER_FILL: &str = "#C7D2FE";
pub(crate) const SG_STEP_FILL: &str = "#E0E7FF";
pub(crate) const SG_STEP_BORDER: &str = "#818CF8";
pub(crate) const SG_INTERNAL_EDGE_COLOR: &str = "#6366f1";
pub(crate) const SG_PHASE_FONT_SIZE: &str = "10";
pub(crate) const SG_STEP_FONT_SIZE: &str = "9";

const DEFAULT_FILL: &str = "#F8FAFC";

pub(crate) const RUN_FILL: &str = "#F1F5F9";

pub(crate) fn get_fill_color(name: &str) -> &'static str {
    let name_lower = name.to_lowercase();
    if name_lower.contains("producer") {
        "#EFF6FF"
    } else if name_lower.contains("consumer") || name_lower.contains("worker") {
        "#ECFDF5"
    } else if name_lower.contains("partial") {
        "#FFFBEB"
    } else if name_lower.contains("total") {
        "#FFF1F2"
    } else {
        DEFAULT_FILL
    }
}

pub(crate) fn escape_dot_label(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('<', "\\<")
        .replace('>', "\\>")
        .replace('{', "\\{")
        .replace('}', "\\}")
}

pub(crate) fn clean_id(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect()
}

#[allow(clippy::expect_used)]
pub(crate) fn canonical_json(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        _ => serde_json::to_string(v).expect("serializing JSON value to string should not fail"),
    }
}

#[allow(clippy::expect_used)]
pub(crate) fn smart_truncate(val: &Value, max_len: usize) -> String {
    let mut s = match val {
        Value::String(s) => s.clone(),
        _ => serde_json::to_string(val).expect("serializing JSON value to string should not fail"),
    };

    if s.contains('/') {
        if let Some(filename) = s.split('/').next_back() {
            s = filename.to_string();
        }
    }

    s = s.replace(['[', ']', '\'', '"'], "");

    let char_count = s.chars().count();
    if char_count > max_len {
        let keep = (max_len / 2).saturating_sub(2);
        let start: String = s.chars().take(keep).collect();
        let end: String = s.chars().skip(char_count - keep).collect();
        return format!("{}..{}", start, end);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_fill_color() {
        assert_eq!(get_fill_color("stage-producer-abc"), "#EFF6FF");
        assert_eq!(get_fill_color("stage-consumer-xyz"), "#ECFDF5");
        assert_eq!(get_fill_color("data-worker-123"), "#ECFDF5");
        assert_eq!(get_fill_color("partial-sum-stage"), "#FFFBEB");
        assert_eq!(get_fill_color("total-sum-stage"), "#FFF1F2");
        assert_eq!(get_fill_color("random-stage-name"), DEFAULT_FILL);
        assert_eq!(get_fill_color("STAGE-PRODUCER"), "#EFF6FF");
        assert_eq!(get_fill_color("Stage-Consumer"), "#ECFDF5");
        assert_eq!(get_fill_color(""), DEFAULT_FILL);
    }

    #[test]
    fn test_clean_id() {
        assert_eq!(clean_id("stage-A-producer"), "stageAproducer");
        assert_eq!(clean_id("job@123#test"), "job123test");
        assert_eq!(clean_id("valid_name_123"), "valid_name_123");
        assert_eq!(clean_id(""), "");
        assert_eq!(clean_id("@#$%^&*"), "");
        assert_eq!(clean_id("name"), "name");
    }

    #[test]
    fn test_smart_truncate() {
        let short = Value::String("short".to_string());
        assert_eq!(smart_truncate(&short, 30), "short");

        let long_str = Value::String("a".repeat(50));
        let result = smart_truncate(&long_str, 20);
        assert!(result.len() <= 20);
        assert!(result.contains(".."));

        let path = Value::String("/very/long/path/to/filename.txt".to_string());
        assert_eq!(smart_truncate(&path, 30), "filename.txt");

        let arr = Value::String("[1, 2, 3]".to_string());
        let res = smart_truncate(&arr, 30);
        assert!(!res.contains('['));
        assert!(!res.contains(']'));

        let quoted = Value::String("'quoted'".to_string());
        let res = smart_truncate(&quoted, 30);
        assert!(!res.contains('\''));

        let num = serde_json::json!(12345);
        assert_eq!(smart_truncate(&num, 30), "12345");

        let exact = Value::String("x".repeat(10));
        assert_eq!(smart_truncate(&exact, 10), "xxxxxxxxxx");

        let boundary = Value::String("a".repeat(11));
        let res = smart_truncate(&boundary, 10);
        assert!(res.len() <= 10);
    }
}
