//! The pin end to end, with no network: a `Source` over the fixture files
//! stands in for the origin. The fixtures are the real `common-ui@b9f049d05d44`
//! files and the real root manifest, so the digests exercised are the ones a
//! consumer stamps.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use common_ui_build::fetch::{self, Source};
use common_ui_build::{
    csp, read_prefix, sha384, sri_table, stamp, Error, Files, Manifest, Markers, Pin, MANIFEST,
    MARKERS, OUT_MANIFEST, SHELL, TYPES,
};

const PREFIX: &str = "common-ui@b9f049d05d44";

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn files() -> Files {
    Files {
        shell: fixture(SHELL),
        markers: fixture(MARKERS),
        dts: fixture(TYPES),
    }
}

fn manifest() -> Manifest {
    Manifest::parse(&fixture(MANIFEST), "fixture manifest").expect("the fixture manifest parses")
}

fn pin() -> Pin {
    Pin::from_parts(PREFIX, manifest(), files()).expect("the fixtures verify")
}

/// An origin in a map. Counts GETs so a test can prove the cache was used.
struct Fake {
    paths: BTreeMap<String, Vec<u8>>,
    gets: RefCell<usize>,
}

impl Fake {
    /// The origin as it is today: a root manifest, no per-prefix one.
    fn root_only() -> Self {
        let mut paths = BTreeMap::new();
        paths.insert(MANIFEST.to_owned(), fixture(MANIFEST).into_bytes());
        for name in [SHELL, MARKERS, TYPES] {
            paths.insert(format!("{PREFIX}/{name}"), fixture(name).into_bytes());
        }
        Self {
            paths,
            gets: RefCell::new(0),
        }
    }

    /// The origin as common will publish it: a manifest under the prefix.
    fn per_prefix() -> Self {
        let mut fake = Self::root_only();
        let root = fake.paths.remove(MANIFEST).expect("root");
        fake.paths.insert(format!("{PREFIX}/{MANIFEST}"), root);
        fake
    }
}

impl Source for Fake {
    fn origin(&self) -> &str {
        "https://origin.test"
    }
    fn get(&self, path: &str) -> common_ui_build::Result<Option<Vec<u8>>> {
        *self.gets.borrow_mut() += 1;
        Ok(self.paths.get(path).cloned())
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "common-ui-build-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// The 15 values a consumer stamps, in a consumer's order.
fn values<'a>(sri: &'a dyn Fn(&str) -> String) -> Vec<(&'static str, String)> {
    vec![
        ("title", "t".into()),
        ("app_name", "app".into()),
        ("app_icon", "x".into()),
        ("bar_pages", "<a href=\"/\">Home</a>".into()),
        ("prefix", PREFIX.into()),
        ("page_css", "/pages/p.css".into()),
        ("page_module", "/pages/p.js".into()),
        ("root_class", "wide".into()),
        ("sri_base_css", sri("base.css")),
        ("sri_palette_css", sri("theme-default/palette.css")),
        ("sri_elements_css", sri("elements.css")),
        ("sri_common_ui_js", sri("common-ui.js")),
        ("footer_app", "app".into()),
        ("footer_commit", "abc123".into()),
        ("legal", "internal".into()),
    ]
}

fn stamp_all(pin: &Pin, values: &[(&str, String)]) -> String {
    let borrowed: Vec<(&str, &str)> = values.iter().map(|(m, v)| (*m, v.as_str())).collect();
    stamp(&pin.files.shell, &borrowed)
}

// ---- the pin line -----------------------------------------------------------

#[test]
fn the_prefix_is_one_metadata_line_in_cargo_toml() {
    let toml = "[package]\nname = \"x\"\nversion = \"0.1.0\"\n\n[package.metadata.common-ui]\nprefix = \"common-ui@b9f049d05d44\"\n";
    assert_eq!(read_prefix(toml, Path::new("Cargo.toml")).unwrap(), PREFIX);
}

#[test]
fn a_manifest_without_the_pin_says_what_line_to_add() {
    let toml = "[package]\nname = \"x\"\nversion = \"0.1.0\"\n";
    let err = read_prefix(toml, Path::new("Cargo.toml")).unwrap_err();
    assert!(matches!(err, Error::NoPrefix { .. }), "{err}");
    assert!(
        err.to_string()
            .contains("[package.metadata.common-ui] prefix"),
        "{err}"
    );
}

#[test]
fn a_prefix_that_is_not_common_ui_at_hex_is_refused() {
    for bad in [
        "b9f049d05d44",
        "common-ui@",
        "common-ui@../x",
        "common-ui@B9F049D05D44",
        "common-ui@zz",
    ] {
        let toml = format!("[package]\nname = \"x\"\nversion = \"0.1.0\"\n[package.metadata.common-ui]\nprefix = \"{bad}\"\n");
        let err = read_prefix(&toml, Path::new("Cargo.toml")).unwrap_err();
        assert!(matches!(err, Error::BadPrefix(_)), "{bad}: {err}");
    }
}

// ---- digests ----------------------------------------------------------------

#[test]
fn every_fetched_file_must_match_the_manifest() {
    pin();
    for (name, tamper) in [(SHELL, "shell"), (MARKERS, "markers"), (TYPES, "dts")] {
        let mut f = files();
        match tamper {
            "shell" => f.shell.push(' '),
            "markers" => f.markers.push(' '),
            _ => f.dts.push(' '),
        }
        let err = Pin::from_parts(PREFIX, manifest(), f).unwrap_err();
        assert!(
            matches!(&err, Error::Digest { what, .. } if what == &format!("{PREFIX}/{name}")),
            "{name}: {err}"
        );
    }
}

#[test]
fn a_file_the_manifest_does_not_list_cannot_be_verified() {
    let mut m = manifest();
    m.files.remove(&format!("{PREFIX}/{TYPES}"));
    let err = Pin::from_parts(PREFIX, m, files()).unwrap_err();
    assert!(matches!(err, Error::NotInManifest { .. }), "{err}");
}

#[test]
fn sha384_is_the_integrity_spelling() {
    // 48 bytes → 64 base64 chars, never padded.
    let s = sha384(b"");
    assert!(
        s.starts_with("sha384-") && s.len() == 7 + 64 && !s.ends_with('='),
        "{s}"
    );
    assert_eq!(
        s,
        "sha384-OLBgp1GsljhM2TJ+sbHjaiH9txEUvgdDTAzHv2P24donTt6/529l+9Ua0vFImLlb"
    );
}

// ---- the manifest rule ------------------------------------------------------

#[test]
fn the_per_prefix_manifest_is_taken_when_it_exists() {
    let src = Fake::per_prefix();
    let m = fetch::manifest_for(PREFIX, &src).unwrap();
    assert!(m.files.contains_key(&format!("{PREFIX}/{SHELL}")));
    assert_eq!(*src.gets.borrow(), 1, "one GET, under the prefix");
}

#[test]
fn the_root_manifest_stands_in_only_for_the_prefix_it_publishes() {
    let src = Fake::root_only();
    let m = fetch::manifest_for(PREFIX, &src).unwrap();
    assert_eq!(m.prefix.as_deref(), Some(PREFIX));

    let err = fetch::manifest_for("common-ui@000000000000", &src).unwrap_err();
    match &err {
        Error::NoManifest {
            prefix,
            url,
            published,
            ..
        } => {
            assert_eq!(prefix, "common-ui@000000000000");
            assert_eq!(
                url,
                "https://origin.test/common-ui@000000000000/manifest.json"
            );
            assert_eq!(published, PREFIX);
        }
        other => panic!("expected NoManifest, got {other}"),
    }
    assert!(err.to_string().contains("per-prefix manifest"), "{err}");
}

// ---- the cache --------------------------------------------------------------

#[test]
fn the_first_load_fetches_and_the_second_is_offline() {
    let cache = temp_dir("cache");
    let src = Fake::root_only();
    let first = fetch::load(PREFIX, &cache, &src).unwrap();
    assert_eq!(first.prefix, PREFIX);
    let fetched = *src.gets.borrow();
    assert_eq!(
        fetched, 5,
        "per-prefix manifest (404), root manifest, three files"
    );
    for name in [MANIFEST, SHELL, MARKERS, TYPES] {
        assert!(cache.join(name).is_file(), "{name} cached");
    }
    assert!(
        std::fs::read_dir(&cache).unwrap().all(|e| !e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".tmp")),
        "no temp file left behind"
    );

    let second = fetch::load(PREFIX, &cache, &src).unwrap();
    assert_eq!(
        *src.gets.borrow(),
        fetched,
        "the second load made no request"
    );
    assert_eq!(second.files.shell, first.files.shell);
    assert_eq!(second.manifest.raw, first.manifest.raw);
    let _ = std::fs::remove_dir_all(&cache);
}

#[test]
fn a_tampered_cache_fails_the_build_rather_than_stamping() {
    let cache = temp_dir("tamper");
    let src = Fake::root_only();
    fetch::load(PREFIX, &cache, &src).unwrap();
    let shell = cache.join(SHELL);
    let mut text = std::fs::read_to_string(&shell).unwrap();
    text.push_str("<!-- edited -->");
    std::fs::write(&shell, text).unwrap();

    let err = fetch::load(PREFIX, &cache, &src).unwrap_err();
    assert!(matches!(err, Error::Digest { .. }), "{err}");
    assert!(
        err.to_string().contains(&cache.display().to_string()),
        "names the cache: {err}"
    );
    let _ = std::fs::remove_dir_all(&cache);
}

#[test]
fn a_partial_cache_is_fetched_over() {
    let cache = temp_dir("partial");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join(SHELL), "stale").unwrap();
    let src = Fake::root_only();
    let pin = fetch::load(PREFIX, &cache, &src).unwrap();
    assert!(*src.gets.borrow() > 0);
    assert_eq!(
        std::fs::read_to_string(cache.join(SHELL)).unwrap(),
        pin.files.shell
    );
    let _ = std::fs::remove_dir_all(&cache);
}

// ---- what a build.rs gets out of the pin -----------------------------------

#[test]
fn the_sri_table_is_the_prefix_minus_the_three_embedded_files() {
    let pin = pin();
    let table = pin.sri_table();
    let names: Vec<&str> = table.iter().map(|(n, _)| n.as_str()).collect();
    for linked in [
        "base.css",
        "theme-default/palette.css",
        "elements.css",
        "common-ui.js",
    ] {
        assert!(names.contains(&linked), "{linked} missing from {names:?}");
    }
    for embedded in [SHELL, MARKERS, TYPES] {
        assert!(
            !names.contains(&embedded),
            "{embedded} is embedded, not linked"
        );
    }
    assert!(table.iter().all(|(_, s)| s.starts_with("sha384-")));
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(
        names, sorted,
        "sorted by name so generated files are stable"
    );
    assert_eq!(sri_table(&pin.manifest, PREFIX), table);
    // Nothing from another prefix leaks in.
    assert!(sri_table(&pin.manifest, "common-ui@000000000000").is_empty());
}

#[test]
fn themes_are_derived_from_the_manifest_default_included() {
    let pin = pin();
    let themes = pin.themes();
    let names: Vec<&str> = themes.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"default"), "{names:?}");
    assert!(names.contains(&"ink"), "{names:?}");
    assert_eq!(
        themes.len(),
        16,
        "the fixture prefix ships sixteen palettes"
    );
    for (name, sri) in &themes {
        assert_eq!(pin.sri(&format!("theme-{name}/palette.css")).unwrap(), sri);
    }
}

#[test]
fn the_csp_is_the_shells_with_the_origin_marker_in_it() {
    let pin = pin();
    assert!(pin.csp().contains("{{assets_origin}}"));
    assert_eq!(csp(&pin.files.markers).unwrap(), pin.csp());
    let err =
        Markers::parse(r#"{"build":[],"runtime":[],"csp":"default-src 'self'"}"#).unwrap_err();
    assert!(matches!(err, Error::Markers(_)), "{err}");
    // The `build`/`runtime` arrays are not parsed (R114.2): a file with only
    // the csp is a complete contract to this crate.
    assert_eq!(
        Markers::parse(r#"{"csp":"style-src {{assets_origin}}"}"#)
            .unwrap()
            .csp,
        "style-src {{assets_origin}}"
    );
}

#[test]
fn write_dts_and_write_manifest_land_in_out_dir_verbatim() {
    let out = temp_dir("out");
    std::fs::create_dir_all(&out).unwrap();
    let pin = pin();
    let dts = pin.write_dts(&out).unwrap();
    assert_eq!(dts, out.join(TYPES));
    assert_eq!(
        sha384(&std::fs::read(&dts).unwrap()),
        pin.sri(TYPES).unwrap()
    );
    let m = pin.write_manifest(&out).unwrap();
    assert_eq!(m, out.join(OUT_MANIFEST));
    assert_eq!(std::fs::read_to_string(&m).unwrap(), fixture(MANIFEST));
    let _ = std::fs::remove_dir_all(&out);
}

// ---- stamping ---------------------------------------------------------------
// `[[build]]` markers are filled here, and `upon` refuses an unfilled one by
// name (a build-time error); `{{runtime}}` markers are left for the app's boot
// render (common-templating `Shell`), which refuses an unfilled one by name.

#[test]
fn a_full_stamp_fills_the_build_markers_and_leaves_the_runtime_ones() {
    let pin = pin();
    let sri = |n: &str| pin.sri(n).unwrap().to_owned();
    let html = stamp_all(&pin, &values(&sri));
    // Every runtime `{{marker}}` reaches the per-request render verbatim.
    for runtime in ["assets_origin", "config", "theme_override"] {
        assert!(
            html.contains(&format!("{{{{{runtime}}}}}")),
            "{runtime} survives"
        );
    }
    // Every build `[[marker]]` is gone: the values filled them.
    assert!(
        !html.contains("[["),
        "no build marker may survive a full stamp: {html}"
    );
    assert!(html.contains(&format!("integrity=\"{}\"", sri("base.css"))));
}

#[test]
fn the_runtime_markers_pass_through_stamp_untouched() {
    // `[[ ]]` is the build engine's delimiter; `{{ }}` is not, so the three
    // runtime markers are copied verbatim for common-templating to fill.
    let out = stamp(
        "[[title]] {{assets_origin}} {{config}} {{theme_override}}",
        &[("title", "T")],
    );
    assert_eq!(out, "T {{assets_origin}} {{config}} {{theme_override}}");
}

#[test]
fn an_unfilled_build_marker_is_a_build_error() {
    // A build marker the shell holds but the consumer does not fill (a forgot
    // or a typo) panics the build, naming the marker — not a raw `[[marker]]`
    // shipped to the browser as the old blind replace would have done.
    let prior = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let caught = std::panic::catch_unwind(|| stamp("[[title]] [[forgotten]]", &[("title", "T")]));
    std::panic::set_hook(prior);

    let payload = caught.expect_err("an unfilled [[marker]] must fail the build");
    let msg = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or("");
    assert!(
        msg.contains("forgotten"),
        "the build failure must name the unfilled marker: {msg}"
    );
}
