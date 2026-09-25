//! A consumer in miniature driving `load_from`: precedence, nesting, every
//! generated spelling, legacy overrides, refusals, masking, bool forms. The
//! derive is exercised from OUTSIDE the crate on purpose. Anything asserting
//! a single module's pure helper (a path spelling, a type name) belongs in
//! that module's inline tests, not here.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use common_config::{Config, Deployment, Outcome, Path, Refusal, Root};

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumString, strum::VariantNames)]
enum Class {
    #[strum(serialize = "prod")]
    Prod,
    #[strum(serialize = "dev")]
    Dev,
}

#[derive(Debug, Config)]
#[config(app = "APP", bin = "app")]
struct Top {
    /// The name.
    #[config(required)]
    name: String,
    /// A count.
    #[config(default = "1")]
    count: u32,
    /// Chatty.
    verbose: bool,
    /// A note.
    note: Option<String>,
    /// Maybe.
    maybe: Option<bool>,
    /// A token.
    #[config(secret, default = "s3cret")]
    token: String,
    /// A pin.
    #[config(secret, default = "1234")]
    pin: u32,
    /// Legacy names.
    #[config(env = "LEGACY_MODE", flag = "mode")]
    mode: Option<String>,
    /// Class.
    #[config(default = "dev", accepted = "prod or dev")]
    class: Class,
    /// Upstream.
    #[config(prod_required)]
    upstream: Option<String>,
    /// Stub identity.
    #[config(dev_only)]
    stub_user: Option<String>,
    /// Debug auth.
    #[config(dev_only)]
    no_auth: bool,
    #[config(nested)]
    mid: Mid,
    deployment: Deployment,
    #[config(nested)]
    log: Log,
}

#[derive(Debug, Config)]
struct Mid {
    /// Size.
    #[config(default = "10")]
    size: u64,
    #[config(nested)]
    deep: Deep,
}

#[derive(Debug, Config)]
struct Deep {
    /// Level.
    #[config(default = "7")]
    level: u8,
}

/// Stands in for the logging crate's section, which this crate cannot see.
#[derive(Debug, Config)]
struct Log {
    /// Format.
    #[config(default = "json", env = "LOG_FORMAT")]
    format: String,
    /// Filter.
    #[config(env = "LOG_DESIGNATORS")]
    designators: Option<String>,
}

#[derive(Debug, Config)]
#[config(app = "STRICT")]
struct Strict {
    #[config(nested)]
    inner: StrictInner,
    deployment: Deployment,
    #[config(nested)]
    log: Log,
}

#[derive(Debug, Config)]
struct StrictInner {
    /// Needed.
    #[config(required)]
    key: String,
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// A dev deployment unless the test sets one itself (a later entry wins).
fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    std::iter::once(("DEPLOYMENT_TYPE", "dev"))
        .chain(items.iter().copied())
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn load(args: &[&str], env: &[(&str, &str)]) -> Result<Outcome<Top>, Refusal> {
    common_config::load_from::<Top>(&strings(args), &pairs(env))
}

fn ok(args: &[&str], env: &[(&str, &str)]) -> Top {
    match load(args, env) {
        Ok(Outcome::Config(top)) => top,
        other => panic!("expected a config, got {other:?}"),
    }
}

fn err(args: &[&str], env: &[(&str, &str)]) -> Refusal {
    match load(args, env) {
        Err(refusal) => refusal,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn text(args: &[&str], env: &[(&str, &str)]) -> String {
    match load(args, env) {
        Ok(Outcome::Help(t)) | Ok(Outcome::PrintConfig(t)) => t,
        other => panic!("expected help or print-config text, got {other:?}"),
    }
}

static FILES: AtomicUsize = AtomicUsize::new(0);

fn file(content: &str) -> PathBuf {
    let n = FILES.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("common-config-tests-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{n}.toml"));
    std::fs::write(&path, content).unwrap();
    path
}

fn config_arg(path: &std::path::Path) -> String {
    format!("--config={}", path.display())
}

#[test]
fn precedence_default_when_nothing_sets_it() {
    assert_eq!(ok(&["--name=x"], &[]).count, 1);
}

#[test]
fn precedence_file_beats_default() {
    let f = file("count = 2\n");
    assert_eq!(ok(&["--name=x", &config_arg(&f)], &[]).count, 2);
}

#[test]
fn precedence_env_beats_file() {
    let f = file("count = 2\n");
    let top = ok(&["--name=x", &config_arg(&f)], &[("APP_COUNT", "3")]);
    assert_eq!(top.count, 3);
}

#[test]
fn precedence_arg_beats_env_and_file() {
    let f = file("count = 2\n");
    let top = ok(
        &["--name=x", &config_arg(&f), "--count=4"],
        &[("APP_COUNT", "3")],
    );
    assert_eq!(top.count, 4);
}

#[test]
fn precedence_is_per_field_not_per_source() {
    let f = file("count = 2\n[mid]\nsize = 20\n");
    let top = ok(
        &["--name=x", &config_arg(&f)],
        &[("APP_MID__DEEP__LEVEL", "3")],
    );
    assert_eq!((top.count, top.mid.size, top.mid.deep.level), (2, 20, 3));
}

#[test]
fn later_arg_wins_over_earlier_arg() {
    assert_eq!(ok(&["--name=x", "--count=5", "--count=6"], &[]).count, 6);
}

#[test]
fn nested_three_deep_from_file_tables() {
    let f = file("[mid.deep]\nlevel = 3\n");
    assert_eq!(ok(&["--name=x", &config_arg(&f)], &[]).mid.deep.level, 3);
}

#[test]
fn nested_three_deep_from_env() {
    let top = ok(&["--name=x"], &[("APP_MID__DEEP__LEVEL", "4")]);
    assert_eq!(top.mid.deep.level, 4);
}

#[test]
fn nested_three_deep_from_flag() {
    assert_eq!(
        ok(&["--name=x", "--mid--deep--level=5"], &[])
            .mid
            .deep
            .level,
        5
    );
}

#[test]
fn nested_defaults_apply_without_any_source() {
    let top = ok(&["--name=x"], &[]);
    assert_eq!((top.mid.size, top.mid.deep.level), (10, 7));
}

#[test]
fn schema_spells_every_field_three_ways() {
    let fields = Top::schema(&Path::root());
    let got: Vec<(String, String, String)> = fields
        .iter()
        .map(|f| (f.flag(), f.env("APP"), f.toml()))
        .collect();
    let want: Vec<(String, String, String)> = [
        ("--name", "APP_NAME", "name"),
        ("--count", "APP_COUNT", "count"),
        ("--verbose", "APP_VERBOSE", "verbose"),
        ("--note", "APP_NOTE", "note"),
        ("--maybe", "APP_MAYBE", "maybe"),
        ("--token", "APP_TOKEN", "token"),
        ("--pin", "APP_PIN", "pin"),
        ("--mode", "LEGACY_MODE", "mode"),
        ("--class", "APP_CLASS", "class"),
        ("--upstream", "APP_UPSTREAM", "upstream"),
        ("--stub-user", "APP_STUB_USER", "stub_user"),
        ("--no-auth", "APP_NO_AUTH", "no_auth"),
        ("--mid--size", "APP_MID__SIZE", "mid.size"),
        (
            "--mid--deep--level",
            "APP_MID__DEEP__LEVEL",
            "mid.deep.level",
        ),
        ("--deployment", "DEPLOYMENT_TYPE", "deployment"),
        ("--log--format", "LOG_FORMAT", "log.format"),
        ("--log--designators", "LOG_DESIGNATORS", "log.designators"),
    ]
    .iter()
    .map(|(a, b, c)| (a.to_string(), b.to_string(), c.to_string()))
    .collect();
    assert_eq!(got, want);
}

#[test]
fn schema_help_comes_from_doc_comments() {
    let fields = Top::schema(&Path::root());
    let level = fields
        .iter()
        .find(|f| f.toml() == "mid.deep.level")
        .unwrap();
    assert_eq!(level.help, "Level.");
}

#[test]
fn legacy_env_override_reads_the_bare_name() {
    let top = ok(&["--name=x"], &[("LEGACY_MODE", "fast")]);
    assert_eq!(top.mode.as_deref(), Some("fast"));
}

#[test]
fn legacy_env_override_ignores_the_generated_name() {
    let top = ok(&["--name=x"], &[("APP_MODE", "fast")]);
    assert_eq!(top.mode, None);
}

#[test]
fn legacy_flag_override_replaces_the_generated_flag() {
    assert_eq!(
        ok(&["--name=x", "--mode=slow"], &[]).mode.as_deref(),
        Some("slow")
    );
    let refusal = err(&["--name=x", "--mode-x=slow"], &[]);
    assert!(refusal.detail.unwrap().contains("--mode-x"));
}

#[test]
fn unknown_toml_key_refuses_naming_it() {
    let f = file("nme = \"typo\"\n[mid]\nsize = 1\n");
    let refusal = err(&["--name=x", &config_arg(&f)], &[]);
    assert_eq!(refusal.variable, "--config");
    assert_eq!(refusal.value, f.display().to_string());
    assert_eq!(refusal.detail.as_deref(), Some("unknown keys: nme"));
}

#[test]
fn unknown_toml_nested_key_is_named_with_its_table() {
    let f = file("[mid.deep]\nlevle = 1\n");
    let refusal = err(&["--name=x", &config_arg(&f)], &[]);
    assert_eq!(
        refusal.detail.as_deref(),
        Some("unknown keys: mid.deep.levle")
    );
}

#[test]
fn unknown_flag_refuses_naming_it() {
    let refusal = err(&["--name=x", "--bogus", "--also=1"], &[]);
    assert_eq!(refusal.variable, "arguments");
    assert_eq!(
        refusal.detail.as_deref(),
        Some("unknown: --bogus, --also=1")
    );
}

#[test]
fn positional_argument_refuses() {
    let refusal = err(&["--name=x", "stray"], &[]);
    assert_eq!(refusal.detail.as_deref(), Some("unknown: stray"));
}

#[test]
fn flag_without_value_refuses() {
    let refusal = err(&["--name"], &[]);
    assert_eq!(refusal.variable, "--name");
    assert_eq!(
        refusal.detail.as_deref(),
        Some("flag given without a value")
    );
}

#[test]
fn flag_value_may_not_look_like_a_flag() {
    let refusal = err(&["--name", "--count=1"], &[]);
    assert_eq!(refusal.variable, "--name");
}

#[test]
fn secret_is_masked_in_print_config() {
    let out = text(&["--name=x", "--token=hunter2", "--print-config"], &[]);
    assert!(!out.contains("hunter2"));
    assert!(out.contains("token"));
    assert!(out.contains("****"));
}

#[test]
fn secret_values_still_load_unmasked() {
    let top = ok(&["--name=x", "--token=hunter2"], &[("APP_PIN", "42")]);
    assert_eq!((top.token.as_str(), top.pin), ("hunter2", 42));
}

#[test]
fn secret_is_masked_in_a_wrong_type_refusal() {
    let refusal = err(&["--name=x", "--pin=letters"], &[]);
    assert_eq!(refusal.variable, "--pin");
    assert_eq!(refusal.value, "****");
}

#[test]
fn bool_accepts_true_forms() {
    for form in ["true", "1", "yes", "TRUE", "Yes", "on"] {
        assert!(
            ok(&["--name=x"], &[("APP_VERBOSE", form)]).verbose,
            "{form}"
        );
    }
}

#[test]
fn bool_accepts_false_forms() {
    for form in ["false", "0", "no", "FALSE", "No", "off"] {
        assert!(
            !ok(&["--name=x"], &[("APP_VERBOSE", form)]).verbose,
            "{form}"
        );
    }
}

#[test]
fn bool_bare_flag_means_true() {
    assert!(ok(&["--name=x", "--verbose"], &[]).verbose);
}

#[test]
fn bool_flag_takes_an_inline_value() {
    assert!(!ok(&["--name=x", "--verbose=no"], &[]).verbose);
}

#[test]
fn bool_defaults_to_false() {
    assert!(!ok(&["--name=x"], &[]).verbose);
}

#[test]
fn bool_invalid_refuses_naming_the_forms() {
    let refusal = err(&["--name=x"], &[("APP_VERBOSE", "maybe")]);
    assert_eq!(refusal.variable, "APP_VERBOSE");
    assert_eq!(refusal.value, "maybe");
    assert_eq!(refusal.accepted, "true, false, 1, 0, yes, no, on or off");
}

#[test]
fn option_bool_is_none_until_set() {
    assert_eq!(ok(&["--name=x"], &[]).maybe, None);
    assert_eq!(ok(&["--name=x", "--maybe"], &[]).maybe, Some(true));
    assert_eq!(ok(&["--name=x", "--maybe=0"], &[]).maybe, Some(false));
}

#[test]
fn wrong_type_from_arg_names_flag_text_and_error() {
    let refusal = err(&["--name=x", "--count=abc"], &[]);
    assert_eq!(refusal.variable, "--count");
    assert_eq!(refusal.value, "abc");
    assert_eq!(refusal.accepted, "a u32");
    assert_eq!(
        refusal.detail.as_deref(),
        Some("set by arg --count: invalid digit found in string")
    );
}

#[test]
fn wrong_type_from_env_names_the_variable() {
    let refusal = err(&["--name=x"], &[("APP_MID__SIZE", "-1")]);
    assert_eq!(refusal.variable, "APP_MID__SIZE");
    assert_eq!(refusal.value, "-1");
    assert!(refusal
        .detail
        .unwrap()
        .starts_with("set by env APP_MID__SIZE: "));
}

#[test]
fn wrong_type_from_file_names_file_and_key() {
    let f = file("[mid.deep]\nlevel = 300\n");
    let refusal = err(&["--name=x", &config_arg(&f)], &[]);
    assert_eq!(refusal.variable, format!("{}: mid.deep.level", f.display()));
    assert_eq!(refusal.value, "300");
}

#[test]
fn strum_enum_parses_and_refuses_with_declared_accepted() {
    assert_eq!(ok(&["--name=x", "--class=prod"], &[]).class, Class::Prod);
    let refusal = err(&["--name=x", "--class=staging"], &[]);
    assert_eq!(refusal.accepted, "prod or dev");
    assert_eq!(refusal.value, "staging");
}

#[test]
fn deployment_and_log_load_under_their_bare_env_names_and_the_root_exposes_them() {
    let top = ok(
        &["--name=x"],
        &[
            ("DEPLOYMENT_TYPE", "prod"),
            ("APP_UPSTREAM", "https://up"),
            ("LOG_FORMAT", "human"),
            ("LOG_DESIGNATORS", "auth=debug"),
        ],
    );
    assert_eq!(top.deployment, Deployment::Prod);
    assert_eq!(top.log.format, "human");
    assert_eq!(top.log.designators.as_deref(), Some("auth=debug"));
    assert_eq!(top.deployment(), Deployment::Prod);
    assert_eq!(top.log().format, "human");
}

#[test]
fn deployment_unset_refuses_naming_every_spelling() {
    let refusal = common_config::load_from::<Top>(&strings(&["--name=x"]), &[]).unwrap_err();
    assert_eq!(refusal.variable, "DEPLOYMENT_TYPE");
    assert_eq!(refusal.value, "unset");
    assert!(refusal
        .accepted
        .contains("--deployment on the command line"));
    assert!(refusal
        .accepted
        .contains("`deployment = …` at the top level of the config file"));
}

#[test]
fn deployment_and_log_are_also_reachable_by_flag_and_file() {
    let f = file("[log]\nformat = \"human\"\n");
    let top = ok(
        &[
            "--name=x",
            &config_arg(&f),
            "--deployment=prod",
            "--upstream=u",
        ],
        &[],
    );
    assert_eq!(top.deployment, Deployment::Prod);
    assert_eq!(top.log.format, "human");
}

#[test]
fn a_bad_deployment_type_refuses_in_the_shared_words() {
    let refusal = err(&["--name=x"], &[("DEPLOYMENT_TYPE", "staging")]);
    assert_eq!(refusal.variable, "DEPLOYMENT_TYPE");
    assert_eq!(refusal.value, "staging");
    assert_eq!(refusal.accepted, common_config::DEPLOYMENT_ACCEPTED);
    let refusal = err(&["--name=x"], &[("DEPLOYMENT_TYPE", "")]);
    assert_eq!(
        refusal.value, "",
        "an empty value is set, and set-but-invalid refuses"
    );
}

#[test]
fn the_deployment_help_is_the_shared_text_unless_the_root_documents_it() {
    let field = Top::schema(&Path::root())
        .into_iter()
        .find(|f| f.toml() == "deployment")
        .unwrap();
    assert_eq!(field.help, common_config::DEPLOYMENT_HELP);
    assert_eq!(field.presence, common_config::Presence::Required);
}

#[test]
fn missing_required_names_all_three_spellings() {
    let refusal = err(&[], &[]);
    assert_eq!(refusal.variable, "APP_NAME");
    assert_eq!(refusal.value, "unset");
    assert!(refusal.accepted.contains("APP_NAME in the environment"));
    assert!(refusal.accepted.contains("--name on the command line"));
    assert!(refusal
        .accepted
        .contains("`name = …` at the top level of the config file"));
}

#[test]
fn required_nested_loads_when_set() {
    let env = pairs(&[("STRICT_INNER__KEY", "k")]);
    match common_config::load_from::<Strict>(&[], &env).unwrap() {
        Outcome::Config(strict) => assert_eq!(strict.inner.key, "k"),
        other => panic!("expected a config, got {other:?}"),
    }
}

#[test]
fn missing_required_nested_names_its_table() {
    let refusal = common_config::load_from::<Strict>(&[], &pairs(&[])).unwrap_err();
    assert_eq!(refusal.variable, "STRICT_INNER__KEY");
    assert!(refusal
        .accepted
        .contains("--inner--key on the command line"));
    assert!(refusal
        .accepted
        .contains("`key = …` under [inner] in the config file"));
}

#[test]
fn optional_field_is_none_until_set() {
    assert_eq!(ok(&["--name=x"], &[]).note, None);
    assert_eq!(
        ok(&["--name=x", "--note", "hi"], &[]).note.as_deref(),
        Some("hi")
    );
}

#[test]
fn help_lists_every_field_grouped_by_table() {
    let out = text(&["--help"], &[]);
    for field in Top::schema(&Path::root()) {
        assert!(out.contains(&field.flag()), "{}", field.flag());
        assert!(out.contains(&field.env("APP")), "{}", field.env("APP"));
        assert!(out.contains(field.help), "{}", field.help);
    }
    let top = out.find("top level").unwrap();
    let mid = out.find("[mid]").unwrap();
    let deep = out.find("[mid.deep]").unwrap();
    let log = out.find("[log]").unwrap();
    assert!(top < mid && mid < deep && deep < log);
    assert!(out.contains("APP_CONFIG"));
    assert!(out.contains("default s3cret, secret"));
    assert!(out.contains("  required"));
    assert!(out.contains("default 7"));
}

#[test]
fn help_wins_over_a_missing_required_and_an_unknown_flag() {
    assert!(text(&["--bogus", "-h"], &[]).starts_with("app:"));
}

#[test]
fn print_config_shows_value_and_origin_per_field() {
    let f = file("count = 2\n");
    let out = text(
        &["--name=x", &config_arg(&f), "--print-config"],
        &[("APP_MID__SIZE", "30")],
    );
    assert!(out.starts_with(&format!("config file: {}\n", f.display())));
    let line = |key: &str| {
        out.lines()
            .find(|l| l.starts_with(&format!("{key} ")))
            .unwrap_or_else(|| panic!("no line for {key} in:\n{out}"))
            .to_string()
    };
    assert!(line("name").ends_with("arg --name"));
    assert!(line("count").ends_with(&format!("file {}", f.display())));
    assert!(line("mid.size").ends_with("env APP_MID__SIZE"));
    assert!(line("mid.deep.level").ends_with("default"));
    assert!(line("note").contains("<unset>"));
    assert!(line("deployment").ends_with("env DEPLOYMENT_TYPE"));
}

#[test]
fn print_config_does_not_require_the_required_fields() {
    let out = text(&["--print-config"], &[]);
    assert!(out.contains("name"));
    assert!(out.contains("<unset>"));
    assert!(out.contains("required"));
}

#[test]
fn config_path_comes_from_app_config_env() {
    let f = file("count = 9\n");
    let top = ok(&["--name=x"], &[("APP_CONFIG", &f.display().to_string())]);
    assert_eq!(top.count, 9);
}

#[test]
fn config_flag_beats_app_config_env() {
    let from_flag = file("count = 1\n");
    let from_env = file("count = 2\n");
    let top = ok(
        &["--name=x", &config_arg(&from_flag)],
        &[("APP_CONFIG", &from_env.display().to_string())],
    );
    assert_eq!(top.count, 1);
}

#[test]
fn missing_config_file_refuses_naming_the_path() {
    let path = std::env::temp_dir().join("common-config-does-not-exist.toml");
    let refusal = err(&["--name=x", &config_arg(&path)], &[]);
    assert_eq!(refusal.variable, "--config");
    assert_eq!(refusal.value, path.display().to_string());
    assert!(refusal.detail.unwrap().starts_with("cannot read: "));
    let refusal = err(
        &["--name=x"],
        &[("APP_CONFIG", &path.display().to_string())],
    );
    assert_eq!(refusal.variable, "APP_CONFIG");
}

#[test]
fn unreadable_config_file_refuses() {
    let dir = std::env::temp_dir();
    let refusal = err(&["--name=x", &config_arg(&dir)], &[]);
    assert_eq!(refusal.variable, "--config");
    assert!(refusal.detail.unwrap().starts_with("cannot read: "));
}

#[test]
fn malformed_config_file_refuses_with_the_line() {
    let f = file("count = 1\n[mid\nsize = 2\n");
    let refusal = err(&["--name=x", &config_arg(&f)], &[]);
    assert_eq!(refusal.accepted, "a well-formed TOML file");
    assert!(refusal.detail.unwrap().ends_with("(line 2)"));
}

#[test]
fn array_value_in_config_file_refuses() {
    let f = file("count = [1, 2]\n");
    let refusal = err(&["--name=x", &config_arg(&f)], &[]);
    assert_eq!(refusal.detail.as_deref(), Some("not a scalar: count"));
}

#[test]
fn toml_scalars_of_any_type_become_text() {
    let f = file("count = 5\nverbose = true\nname = \"from-file\"\n[mid]\nsize = 1\n");
    let top = ok(&[&config_arg(&f)], &[]);
    assert_eq!(
        (top.count, top.verbose, top.name.as_str()),
        (5, true, "from-file")
    );
}

#[test]
fn print_config_names_the_env_var_when_no_file_is_configured() {
    let out = text(&["--name=x", "--print-config"], &[]);
    assert!(out.starts_with("config file: none (--config or APP_CONFIG to set one)"));
}

#[test]
fn classes_are_data_on_the_schema_and_print_in_help() {
    let fields = Top::schema(&Path::root());
    let class_of = |key: &str| fields.iter().find(|f| f.toml() == key).unwrap().class;
    assert_eq!(class_of("upstream"), common_config::Class::ProdRequired);
    assert_eq!(class_of("stub_user"), common_config::Class::DevOnly);
    assert_eq!(class_of("no_auth"), common_config::Class::DevOnly);
    assert_eq!(class_of("name"), common_config::Class::Neutral);
    let out = text(&["--help"], &[]);
    let tail = |flag: &str| {
        out.lines()
            .find(|l| l.trim_start().starts_with(flag))
            .unwrap_or_else(|| panic!("no help line for {flag}:\n{out}"))
            .trim_end()
            .to_owned()
    };
    assert!(tail("--upstream").ends_with("prod-required"), "{out}");
    assert!(tail("--stub-user").ends_with("dev-only"), "{out}");
    assert!(tail("--no-auth").ends_with("dev-only"), "{out}");
}

#[test]
fn under_dev_the_classes_do_not_bite() {
    let top = ok(
        &["--name=x", "--stub-user=alice", "--no-auth"],
        &[("DEPLOYMENT_TYPE", "dev")],
    );
    assert_eq!(top.upstream, None);
    assert_eq!(top.stub_user.as_deref(), Some("alice"));
    assert!(top.no_auth);
}

#[test]
fn under_prod_a_prod_required_value_must_be_set() {
    let refusal = err(&["--name=x"], &[("DEPLOYMENT_TYPE", "prod")]);
    assert_eq!(refusal.variable, "APP_UPSTREAM");
    assert_eq!(refusal.value, "unset");
    assert!(refusal.accepted.contains("--upstream"), "{refusal:?}");
    assert_eq!(
        refusal.detail.as_deref(),
        Some("prod-required and unset under DEPLOYMENT_TYPE=prod")
    );
    let top = ok(
        &["--name=x", "--upstream=https://up"],
        &[("DEPLOYMENT_TYPE", "prod")],
    );
    assert_eq!(top.upstream.as_deref(), Some("https://up"));
}

#[test]
fn under_prod_a_dev_only_value_must_not_be_set() {
    let prod = [("DEPLOYMENT_TYPE", "prod"), ("APP_UPSTREAM", "https://up")];
    let refusal = err(&["--name=x", "--stub-user=alice"], &prod);
    assert_eq!(refusal.variable, "APP_STUB_USER");
    assert_eq!(refusal.value, "alice");
    assert_eq!(
        refusal.detail.as_deref(),
        Some("dev-only and set under DEPLOYMENT_TYPE=prod")
    );
    let refusal = err(&["--name=x", "--no-auth"], &prod);
    assert_eq!(refusal.variable, "APP_NO_AUTH");
    assert!(!ok(&["--name=x", "--no-auth=false"], &prod).no_auth);
}
