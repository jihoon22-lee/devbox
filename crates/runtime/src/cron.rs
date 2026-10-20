//! Parsing policy ported from v0.9.0 runtime-engine/core/cron.rs.
//! Occurrence enumeration and explicit DST resolution are added in Task 5.
use croner::{
    Cron,
    errors::CronError,
    parser::{CronParser, Seconds, Year},
};

#[derive(Debug, Clone)]
pub struct CronSchedule {
    pub(crate) cron: Cron,
    normalized_expression: String,
}

impl CronSchedule {
    pub fn parse(expression: &str) -> Result<Self, CronError> {
        let expression = expression.trim();
        reject_unsupported_syntax(expression)?;
        let parser = CronParser::builder()
            .seconds(Seconds::Optional)
            .year(Year::Disallowed)
            .dom_and_dow(false)
            .build();
        let cron = parser.parse(expression)?;
        let normalized_expression = if expression.split_whitespace().count() == 5 {
            format!("0 {}", cron.as_str())
        } else {
            cron.as_str().to_string()
        };
        Ok(Self {
            cron,
            normalized_expression,
        })
    }
    pub fn normalized_expression(&self) -> &str {
        &self.normalized_expression
    }
    pub fn parsed_cron(&self) -> &Cron {
        &self.cron
    }
}

pub fn validate_cron(expression: &str) -> Result<(), CronError> {
    CronSchedule::parse(expression).map(|_| ())
}

fn reject_unsupported_syntax(expression: &str) -> Result<(), CronError> {
    if expression.is_empty() {
        return Err(CronError::EmptyPattern);
    }
    if expression.contains('@') {
        return Err(CronError::InvalidPattern(
            "cron macros (including @reboot) are not supported".into(),
        ));
    }
    let fields: Vec<&str> = expression.split_whitespace().collect();
    if matches!(fields.len(), 5 | 6) && fields[fields.len() - 1].starts_with('+') {
        return Err(CronError::InvalidPattern(
            "the day-of-month/day-of-week relation must use POSIX OR semantics".into(),
        ));
    }
    Ok(())
}
