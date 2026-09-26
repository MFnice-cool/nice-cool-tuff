use pest::Parser;
use pest_derive::Parser;
use serde_json::{Map, Value};

#[derive(Parser)]
#[grammar = "ftb.pest"]
pub struct FtbParser;

pub fn parse_snbt_to_json(input: &str) -> Result<Value, String> {
    let mut parsed = FtbParser::parse(Rule::file, input)
        .map_err(|e| format!("SNBT parse error: {}", e))?;

    let file_pair = parsed.next().unwrap();

    let mut root_map = Map::new();

    for pair in file_pair.into_inner() {
        match pair.as_rule() {
            Rule::object => {
                return Ok(parse_value(pair));
            }
            Rule::pair => {
                let (k, v) = parse_pair(pair);
                root_map.insert(k, v);
            }
            Rule::EOI => break,
            _ => {}
        }
    }

    Ok(Value::Object(root_map))
}

/// Which on-disk quest file format a chunk of text is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestFileFormat {
    /// Legacy FTB Quests SNBT (< 26.1.2.1).
    Snbt,
    /// FTB Quests JSON5 (26.1.2.1+): "Goodbye SNBT and hello JSON5!".
    Json5,
}

/// Detect the quest file format from its content (not the extension —
/// the extension is authoritative in practice, but this is a robust
/// fallback and also powers tests).
///
/// SNBT emits `d`-suffixed doubles (`x: 0.0d`), typed arrays (`[I;…]`) and
/// unquoted keys — none of which are valid JSON5. JSON5 files start with
/// `{` and parse cleanly as JSON5. We try JSON5 first: a strict JSON5
/// parse of an SNBT document fails on the `d` suffixes, while a real JSON5
/// chapter parses fine.
pub fn detect_format(input: &str) -> QuestFileFormat {
    let trimmed = input.trim_start();
    if !trimmed.starts_with('{') {
        // SNBT from FTB always starts with the root object.
        return QuestFileFormat::Snbt;
    }
    if json5::from_str::<Value>(input).is_ok() {
        QuestFileFormat::Json5
    } else {
        QuestFileFormat::Snbt
    }
}

/// Parse a quest chapter file in either format into the unified JSON model
/// the editor uses. Format detection is content-based.
pub fn parse_quest_file_to_json(input: &str) -> Result<Value, String> {
    match detect_format(input) {
        QuestFileFormat::Json5 => {
            json5::from_str::<Value>(input).map_err(|e| format!("JSON5 parse error: {}", e))
        }
        QuestFileFormat::Snbt => parse_snbt_to_json(input),
    }
}

/// Serialize the unified JSON model back to the given format. SNBT keeps
/// the existing writer; JSON5 uses trailing commas-friendly pretty output.
pub fn json_to_quest_file(value: &Value, format: QuestFileFormat) -> Result<String, String> {
    match format {
        QuestFileFormat::Snbt => Ok(json_to_snbt(value)),
        QuestFileFormat::Json5 => serde_json::to_string_pretty(value)
            .map_err(|e| format!("JSON serialize error: {}", e)),
    }
}

fn parse_pair(pair: pest::iterators::Pair<Rule>) -> (String, Value) {
    let mut inner = pair.into_inner();
    let key_pair = inner.next().unwrap();
    let value_pair = inner.next().unwrap();

    let key_rule = key_pair.clone().into_inner().next().unwrap().as_rule();
    let key = match key_rule {
        Rule::string => parse_string(key_pair.into_inner().next().unwrap()),
        _ => key_pair.as_str().to_string(),
    };

    (key, parse_value(value_pair))
}

fn parse_value(pair: pest::iterators::Pair<Rule>) -> Value {
    match pair.as_rule() {
        Rule::object => {
            let mut map = Map::new();
            for inner_pair in pair.into_inner() {
                let (k, v) = parse_pair(inner_pair);
                map.insert(k, v);
            }
            Value::Object(map)
        }
        Rule::array => {
            let mut arr = Vec::new();
            for inner_pair in pair.into_inner() {
                arr.push(parse_value(inner_pair));
            }
            Value::Array(arr)
        }
        Rule::string => Value::String(parse_string(pair)),
        Rule::unquoted_string => Value::String(pair.as_str().to_string()),
        Rule::boolean => Value::Bool(pair.as_str() == "true"),
        Rule::number => {
            let num_str = pair.as_str();
            let clean_num = num_str.trim_end_matches(|c: char| matches!(c, 'b' | 'B' | 's' | 'S' | 'l' | 'L' | 'f' | 'F' | 'd' | 'D'));

            if let Ok(n) = clean_num.parse::<i64>() {
                Value::Number(n.into())
            } else if let Ok(f) = clean_num.parse::<f64>() {
                Value::Number(serde_json::Number::from_f64(f).unwrap())
            } else {
                Value::String(num_str.to_string())
            }
        }
        _ => Value::Null,
    }
}

fn parse_string(pair: pest::iterators::Pair<Rule>) -> String {
    let inner = pair.into_inner().next().unwrap();
    inner.as_str().replace("\\\"", "\"")
}

pub fn json_to_snbt(value: &Value) -> String {
    to_snbt_string(value, 0)
}

fn to_snbt_string(value: &Value, indent: usize) -> String {
    let current_pad = "  ".repeat(indent);
    let inner_pad = "  ".repeat(indent + 1);

    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => {
            if n.is_f64() {
                format!("{}d", n)
            } else {
                n.to_string()
            }
        }
        Value::String(s) => {
            if s.starts_with("[I;") || s.starts_with("[B;") || s.starts_with("[L;") {
                return s.clone();
            }
            format!("\"{}\"", s.replace("\"", "\\\""))
        }
        Value::Array(arr) => {
            if arr.is_empty() {
                return "[]".to_string();
            }
            let mut out = String::from("[\n");
            for val in arr {
                out.push_str(&inner_pad);
                out.push_str(&to_snbt_string(val, indent + 1));
                out.push('\n');
            }
            out.push_str(&current_pad);
            out.push(']');
            out
        }
        Value::Object(map) => {
            if let (Some(Value::String(ty)), Some(Value::Array(items))) =
                (map.get("__snbtArray"), map.get("values"))
            {
                if ty.len() == 1 && "BILFD".contains(ty.as_str()) {
                    if items.is_empty() {
                        return format!("[{ty};]");
                    }
                    let mut out = format!("[{ty};\n");
                    for val in items {
                        out.push_str(&inner_pad);
                        out.push_str(&to_snbt_string(val, indent + 1));
                        out.push('\n');
                    }
                    out.push_str(&current_pad);
                    out.push(']');
                    return out;
                }
            }
            if map.is_empty() {
                return "{}".to_string();
            }
            let mut out = String::from("{\n");
            for (k, v) in map {
                out.push_str(&inner_pad);
                if is_valid_unquoted_key(k) {
                    out.push_str(k);
                } else {
                    out.push_str(&format!("\"{}\"", k.replace("\"", "\\\"")));
                }
                out.push_str(": ");
                out.push_str(&to_snbt_string(v, indent + 1));
                out.push('\n');
            }
            out.push_str(&current_pad);
            out.push('}');
            out
        }
    }
}

fn is_valid_unquoted_key(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_to_snbt_emits_true_false() {
        let out = json_to_snbt(&json!({ "a": true, "b": false }));
        assert!(out.contains("true"), "{out}");
        assert!(out.contains("false"), "{out}");
        assert!(!out.contains("1b"), "{out}");
        assert!(!out.contains("0b"), "{out}");
    }

    #[test]
    fn json_to_snbt_emits_typed_arrays() {
        let v = json!({
            "__snbtArray": "I",
            "values": [1, -2, 3]
        });
        let out = json_to_snbt(&v);
        assert!(out.starts_with("[I;"), "{out}");
        assert!(out.contains('1'), "{out}");
        assert!(out.contains("-2"), "{out}");
    }

    #[test]
    fn detect_format_distinguishes_snbt_from_json5() {
        let snbt = "{\n  id: \"7942A6A571A4C5EB\"\n  x: 0.0d\n  dependencies: [\"6EE7245CA60F0B1C\"]\n}";
        assert_eq!(detect_format(snbt), QuestFileFormat::Snbt);

        let json5 = "{\n  // FTB 26.1.2 comment\n  id: \"7942A6A571A4C5EB\",\n  x: 0.0,\n  dependencies: [\"6EE7245CA60F0B1C\"],\n}";
        assert_eq!(detect_format(json5), QuestFileFormat::Json5);
    }

    #[test]
    fn parse_quest_file_roundtrips_json5() {
        let json5 = "{ id: \"A\", quests: [ { id: \"B\", x: 1.5, tasks: [], rewards: [] } ], }";
        let value = parse_quest_file_to_json(json5).unwrap();
        let out = json_to_quest_file(&value, QuestFileFormat::Json5).unwrap();
        let reparsed = parse_quest_file_to_json(&out).unwrap();
        assert_eq!(value, reparsed);
    }

    #[test]
    fn parse_quest_file_handles_snbt() {
        let snbt = "{\n  id: \"A\"\n  x: 0.0d\n}";
        let value = parse_quest_file_to_json(snbt).unwrap();
        assert_eq!(value["x"].as_f64(), Some(0.0));
        let out = json_to_quest_file(&value, QuestFileFormat::Snbt).unwrap();
        assert!(out.contains("0.0d"), "{out}");
    }
}
