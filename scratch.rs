use serde::Serialize;
use serde::ser::Serializer;

struct Diagnostic {
    rule_id: String,
    severity: u8,
    message: String,
    span: Span,
}
struct Span {
    start: u32,
    end: u32,
}

#[derive(serde::Serialize)]
struct DiagnosticJson<'a> {
    #[serde(rename = "ruleId")]
    rule_id: &'a str,
    severity: &'static str,
    message: &'a str,
    start: u32,
    end: u32,
}

fn severity_str(severity: u8) -> &'static str {
    "error"
}

struct DiagnosticsJsonList<'a>(&'a [Diagnostic]);

impl<'a> Serialize for DiagnosticsJsonList<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_seq(self.0.iter().map(|d| DiagnosticJson {
            rule_id: d.rule_id.as_str(),
            severity: severity_str(d.severity),
            message: d.message.as_str(),
            start: d.span.start,
            end: d.span.end,
        }))
    }
}

fn main() {
    let diags = vec![Diagnostic {
        rule_id: "foo".into(),
        severity: 1,
        message: "bar".into(),
        span: Span { start: 0, end: 1 },
    }];
    let s = serde_json::to_string(&DiagnosticsJsonList(&diags)).unwrap();
    println!("{}", s);
}
