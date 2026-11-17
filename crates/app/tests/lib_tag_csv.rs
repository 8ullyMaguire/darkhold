//! M63-11: the CSV loader must report bad rows without inventing successes.

use clap::Parser;
use darkhold_app::cli::{Cli, Command, LibCommand};
use darkhold_app::lib_tag::{parse_tag_csv, render_report, rollback_sql};

#[test]
fn a_bad_row_is_reported_with_its_line_number_and_good_rows_survive() {
    let input = "work_id,tag\n\
                 11111111-1111-4111-8111-111111111111,dragons\n\
                 not-a-uuid,heroes\n\
                 22222222-2222-4222-8222-222222222222,\"two, words\"\n\
                 33333333-3333-4333-8333-333333333333,dark-mc\n";
    let parsed = parse_tag_csv(input);
    assert_eq!(parsed.rows.len(), 2, "only well-formed rows are applied");
    assert_eq!(parsed.rows[0].tag, "dragons");
    assert_eq!(parsed.rows[1].tag, "dark-mc");
    assert_eq!(parsed.errors.len(), 2, "every bad row has its own report");
    assert_eq!(parsed.errors[0].line, 3);
    assert!(parsed.errors[0].reason.contains("work_id"));
    assert_eq!(parsed.errors[1].line, 4);
    assert!(parsed.errors[1].reason.contains("comma"));

    let report = render_report(&parsed);
    assert!(report.contains("line 3"), "{report}");
    assert!(report.contains("line 4"), "{report}");
    assert!(report.contains("2 applied, 2 failed"), "{report}");

    let sql = rollback_sql(&parsed.rows[0]);
    assert!(sql.contains("DELETE FROM work_tags"));
    assert!(sql.contains("dragons"));
}

#[test]
fn an_unknown_header_is_a_report_not_a_panic() {
    let parsed = parse_tag_csv("work_id,name\nx,y\n");
    assert!(parsed.rows.is_empty());
    assert_eq!(parsed.errors.len(), 1);
    assert_eq!(parsed.errors[0].line, 1);
    assert!(parsed.errors[0].reason.contains("header"));
}

#[test]
fn the_cli_exposes_lib_tag_from_csv() {
    let cli = Cli::try_parse_from([
        "darkhold",
        "lib",
        "tag",
        "--from-csv",
        "/tmp/tags.csv",
        "--rollback",
        "/tmp/tags.rollback.sql",
    ])
    .expect("parses");
    match cli.command {
        Command::Lib(lib) => match lib.command {
            LibCommand::Tag(args) => {
                assert_eq!(args.from_csv.to_string_lossy(), "/tmp/tags.csv");
                assert_eq!(
                    args.rollback.expect("rollback path").to_string_lossy(),
                    "/tmp/tags.rollback.sql"
                );
            }
            other => panic!("wrong lib command: {other:?}"),
        },
        other => panic!("wrong command: {other:?}"),
    }
}
