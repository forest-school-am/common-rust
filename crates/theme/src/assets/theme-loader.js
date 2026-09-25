/* The theme loader (R117, crate-carried delivery).
 *
 * Authored HERE (common-ui-e) and emitted into the common-theme crate by
 * build.sh, exactly as the core assets are — the loader is a built asset that
 * ships INSIDE the crate that serves it, so it cannot drift from the palettes
 * it points at. Apps serve it at /assets/theme-loader.js and the shell links
 * it with a classic (NON-module) <script> in <head>, so it runs SYNCHRONOUSLY
 * before first paint: a default user sees no flash, a non-default user at most
 * a brief one while the swapped sheet loads.
 *
 * It reads the SINGLE theme cookie the picker writes:
 *   les_theme=<name>          -> a crate-bundled palette, app-served, no hash:
 *                                /assets/theme-<name>/palette.css
 *   les_theme=<name>@<hash>   -> a hashed (experimental / newly published)
 *                                palette on Garage, immutable prefix:
 *                                <ASSETS_ORIGIN>/common-ui@<hash>/theme-<name>/palette.css
 *
 * No cookie, or name == "default", leaves the shell's own default <link> in
 * place (the default palette is the crate default, already linked).
 *
 * SECURITY BOUNDARY (tested): both fields are UNTRUSTED. The name must match
 * ^[a-z0-9-]{1,40}$ and the hash ^[0-9a-f]{12}$; a value that fails either is
 * IGNORED (the default stays) and is NEVER reflected into a URL. Those two
 * charsets are what keep the value inside its path segment and the href
 * attribute — no byte that could close the attribute, escape the origin, or
 * traverse the path can survive them.
 *
 * The ASSETS_ORIGIN for the Garage case is not baked into this file (the file
 * is crate-carried and served verbatim, never per-request stamped): it is read
 * from the default palette <link>'s data-assets-origin attribute, which the
 * shell stamps from its {{assets_origin}} runtime marker.
 */
(function () {
  var NAME = /^[a-z0-9-]{1,40}$/;
  var HASH = /^[0-9a-f]{12}$/;

  var match = document.cookie.match(/(?:^|;\s*)les_theme=([^;]*)/);
  if (!match) return; // no cookie -> the shell's default palette stays.

  var raw = match[1];
  var at = raw.indexOf("@");
  var name = at === -1 ? raw : raw.slice(0, at);
  var hash = at === -1 ? "" : raw.slice(at + 1);

  // Validate FIRST. An invalid name, or the default, is no swap.
  if (!NAME.test(name) || name === "default") return;

  var link = document.querySelector("link[data-theme-default]");
  if (!link) return;

  var href;
  if (at === -1) {
    // Crate-bundled: app-served, same origin, no hash.
    href = "/assets/theme-" + name + "/palette.css";
  } else {
    // Hashed: Garage, immutable prefix. A hash that fails the charset is
    // ignored entirely — the default stays, nothing hostile is reflected.
    if (!HASH.test(hash)) return;
    var origin = link.getAttribute("data-assets-origin") || "";
    href = origin + "/common-ui@" + hash + "/theme-" + name + "/palette.css";
    link.crossOrigin = "anonymous";
  }

  // The default's integrity pins the DEFAULT bytes; it cannot apply to another
  // palette, so it is dropped on the swap (the swapped sheet is either
  // same-origin app-served or an immutable content-addressed Garage file).
  link.removeAttribute("integrity");
  link.href = href;
})();
