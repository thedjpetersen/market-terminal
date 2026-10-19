use market_terminal::features::spreadsheet::{
    domain::{parse_formula, CellValue, Workbook, Worksheet},
    Spreadsheet,
};
use std::{
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[test]
fn pathological_formulas_are_errors_in_a_bounded_subprocess() {
    if std::env::var_os("MT_FORMULA_LIMIT_CHILD").is_some() {
        for formula in [
            format!("={}1{}", "(".repeat(10_000), ")".repeat(10_000)),
            format!("={}1", "-".repeat(10_000)),
            format!("={}1{}", "ABS(".repeat(10_000), ")".repeat(10_000)),
            format!("=1{}", "+1".repeat(10_000)),
            format!("=SUM({})", vec!["1"; 10_000].join(",")),
        ] {
            assert!(parse_formula(&formula).is_err());
            let mut sheet = Spreadsheet::new();
            let csv = format!("\"{}\"", formula.replace('"', "\"\""));
            sheet.import_csv(&csv).unwrap();
            assert!(matches!(
                sheet.workbook().active_value("A1".parse().unwrap()),
                CellValue::Error(_)
            ));
        }
        let mut sheet = Worksheet::new("Chain").unwrap();
        for i in 1..100 {
            sheet.set(format!("A{i}").parse().unwrap(), format!("=A{}+1", i + 1));
        }
        sheet.set("A100".parse().unwrap(), "1");
        assert!(matches!(
            sheet.value("A1".parse().unwrap()),
            CellValue::Error(_)
        ));
        let workbook = Workbook::from_sheets(vec![sheet], 0).unwrap();
        assert!(matches!(
            workbook.active_value("A1".parse().unwrap()),
            CellValue::Error(_)
        ));
        assert!(parse_formula("=SUM(A1:A10, 2*3, ABS(-5))").is_ok());
        return;
    }
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "pathological_formulas_are_errors_in_a_bounded_subprocess",
            "--nocapture",
        ])
        .env("MT_FORMULA_LIMIT_CHILD", "1")
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "child failed: {status}");
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("formula test timed out");
        }
        thread::sleep(Duration::from_millis(20));
    }
}
