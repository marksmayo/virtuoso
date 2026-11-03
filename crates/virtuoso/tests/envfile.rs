//! `--env-file`: KEY=VALUE lines, as dotenv files are written.

use virtuoso::envfile::parse_env_file;

#[test]
fn lines_become_pairs_and_comments_and_blanks_are_skipped() {
    let pairs = parse_env_file("# login\nUSER=alice\n\nPASS = s3cret \nexport TOKEN=abc\n").unwrap();
    assert_eq!(
        pairs,
        vec![("USER".into(), "alice".into()), ("PASS".into(), "s3cret".into()), ("TOKEN".into(), "abc".into())]
    );
}

#[test]
fn quoted_values_keep_spaces_and_lose_their_quotes() {
    let pairs = parse_env_file("A=\"two words\"\nB='single # not a comment'\nC=x # trailing comment\n").unwrap();
    assert_eq!(pairs[0].1, "two words");
    assert_eq!(pairs[1].1, "single # not a comment");
    assert_eq!(pairs[2].1, "x");
}

#[test]
fn a_line_without_equals_is_an_error_with_its_line_number() {
    let err = parse_env_file("A=1\noops\n").unwrap_err();
    assert!(err.contains("line 2"), "{err}");
}

#[test]
fn a_line_without_a_value_is_quoted_in_the_error() {
    assert_eq!(parse_env_file("A=1\noops\n").unwrap_err(), "line 2: expected KEY=VALUE, found 'oops'");
}
