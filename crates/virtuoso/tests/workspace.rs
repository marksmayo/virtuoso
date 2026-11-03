//! Which flows a `virtuoso test` run picks, as Maestro picks them.

use virtuoso::workspace::{collect, Selection};
use std::fs;
use std::path::Path;

fn flow(dir: &Path, name: &str, tags: &[&str]) {
    let tags = if tags.is_empty() { String::new() } else { format!("tags:\n{}", tags.iter().map(|t| format!("  - {t}\n")).collect::<String>()) };
    if let Some(p) = Path::new(name).parent() {
        fs::create_dir_all(dir.join(p)).unwrap();
    }
    fs::write(dir.join(name), format!("appId: a\n{tags}---\n- back\n")).unwrap();
}

fn names(sel: &Selection) -> Vec<String> {
    sel.flows.iter().map(|f| f.path.as_ref().unwrap().file_name().unwrap().to_string_lossy().to_string()).collect()
}

#[test]
fn a_folder_runs_its_top_level_flows_in_name_order() {
    let d = tempfile::tempdir().unwrap();
    flow(d.path(), "b.yaml", &[]);
    flow(d.path(), "a.yml", &[]);
    flow(d.path(), "sub/c.yaml", &[]);
    fs::write(d.path().join("notes.txt"), "x").unwrap();
    let sel = collect(&[d.path().to_path_buf()], &[], &[], None).unwrap();
    assert_eq!(names(&sel), vec!["a.yml", "b.yaml"]);
}

#[test]
fn config_yaml_is_not_a_flow_and_its_globs_choose_flows() {
    let d = tempfile::tempdir().unwrap();
    flow(d.path(), "a.yaml", &[]);
    flow(d.path(), "sub/c.yaml", &[]);
    flow(d.path(), "sub/deep/e.yaml", &[]);
    flow(d.path(), "sub/skip.yaml", &[]);
    fs::write(d.path().join("config.yaml"), "flows:\n  - \"sub/**\"\n  - \"!sub/skip.yaml\"\n").unwrap();
    let sel = collect(&[d.path().to_path_buf()], &[], &[], None).unwrap();
    assert_eq!(names(&sel), vec!["c.yaml", "e.yaml"]);
}

#[test]
fn tags_include_and_exclude_from_the_command_line_and_config() {
    let d = tempfile::tempdir().unwrap();
    flow(d.path(), "a.yaml", &["smoke"]);
    flow(d.path(), "b.yaml", &["smoke", "slow"]);
    flow(d.path(), "c.yaml", &[]);
    let sel = collect(&[d.path().to_path_buf()], &["smoke".into()], &["slow".into()], None).unwrap();
    assert_eq!(names(&sel), vec!["a.yaml"]);
    fs::write(d.path().join("config.yaml"), "excludeTags:\n  - smoke\n").unwrap();
    let sel = collect(&[d.path().to_path_buf()], &[], &[], None).unwrap();
    assert_eq!(names(&sel), vec!["c.yaml"]);
}

#[test]
fn execution_order_runs_named_flows_first_in_sequence() {
    let d = tempfile::tempdir().unwrap();
    flow(d.path(), "a.yaml", &[]);
    flow(d.path(), "b.yaml", &[]);
    flow(d.path(), "c.yaml", &[]);
    fs::write(d.path().join("config.yaml"), "executionOrder:\n  continueOnFailure: false\n  flowsOrder:\n    - c\n    - a\n").unwrap();
    let sel = collect(&[d.path().to_path_buf()], &[], &[], None).unwrap();
    assert_eq!(names(&sel), vec!["c.yaml", "a.yaml", "b.yaml"]);
    assert_eq!(sel.sequential, 2);
    assert!(!sel.continue_on_failure);
}

#[test]
fn files_given_directly_run_even_without_matching_tags_config() {
    let d = tempfile::tempdir().unwrap();
    flow(d.path(), "x.yaml", &[]);
    let sel = collect(&[d.path().join("x.yaml")], &[], &[], None).unwrap();
    assert_eq!(names(&sel), vec!["x.yaml"]);
}

#[test]
fn a_flow_that_does_not_parse_is_an_error_naming_it() {
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join("bad.yaml"), "appId: a\n---\n- nope\n").unwrap();
    let err = collect(&[d.path().to_path_buf()], &[], &[], None).unwrap_err();
    assert!(err.to_string().contains("bad.yaml"), "{err}");
}

#[test]
fn a_separate_config_file_can_be_given() {
    let d = tempfile::tempdir().unwrap();
    flow(d.path(), "a.yaml", &["x"]);
    flow(d.path(), "b.yaml", &[]);
    let cfg = d.path().join("ci.yaml");
    fs::write(&cfg, "includeTags: [x]\n").unwrap();
    let sel = collect(&[d.path().to_path_buf()], &[], &[], Some(&cfg)).unwrap();
    assert_eq!(names(&sel), vec!["a.yaml"]);
}

#[test]
fn a_snapshot_of_the_flow_files_changes_when_one_is_edited_added_or_removed() {
    use virtuoso::workspace::snapshot;
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("sub")).unwrap();
    std::fs::write(d.path().join("a.yaml"), "appId: a\n---\n- back\n").unwrap();
    std::fs::write(d.path().join("sub/b.js"), "output.x = 1").unwrap();
    let paths = vec![d.path().to_path_buf()];
    let first = snapshot(&paths);
    assert_eq!(first.len(), 2, "{first:?}");
    assert_eq!(snapshot(&paths), first, "nothing changed");
    std::fs::write(d.path().join("sub/b.js"), "output.x = 22").unwrap();
    let edited = snapshot(&paths);
    assert_ne!(edited, first, "an edit");
    std::fs::write(d.path().join("c.yml"), "appId: a\n---\n").unwrap();
    let added = snapshot(&paths);
    assert_ne!(added, edited, "a new flow");
    std::fs::remove_file(d.path().join("c.yml")).unwrap();
    assert_ne!(snapshot(&paths), added, "a removed flow");
}

#[test]
fn globs_match_paths_without_a_regex_engine() {
    use virtuoso::workspace::glob_matches;
    assert!(glob_matches("*", "login"));
    assert!(!glob_matches("*", "auth/login"), "* stays within a folder");
    assert!(glob_matches("auth/*", "auth/login"));
    assert!(glob_matches("**", "a/b/c"));
    assert!(glob_matches("**/login", "login"), "**/ matches no folder too");
    assert!(glob_matches("**/login", "a/b/login"));
    assert!(glob_matches("a/**/z", "a/z"));
    assert!(glob_matches("a/**/z", "a/b/c/z"));
    assert!(glob_matches("log?n", "login"));
    assert!(!glob_matches("log?n", "log/n"));
    assert!(glob_matches("flow.v2", "flow.v2"));
    assert!(!glob_matches("flow.v2", "flowXv2"), "a dot is a dot");
    assert!(glob_matches("*-smoke", "checkout-smoke"));
    assert!(!glob_matches("*-smoke", "checkout-smoke-2"));
}
