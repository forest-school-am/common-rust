/* The PUBLISHED contract (§12.3's `common-ui.d.ts`), authored rather than
   emitted by tsc. tsc's output is unusable as a published artifact for two
   reasons: it declares `LesPicker extends LitElement` and imports "lit",
   which consumers do not have — R56 FOLDS Lit into common-ui.js — and it
   exposes `static properties` and `#private`, which are not the contract.
   §12.17 says the surface is attributes, properties and events, so that is
   what this declares. build.sh asserts the exported NAMES here match the
   bundle's, so an authored contract cannot drift from the code. */

export interface PickerItem {
  value: string;
  label?: string;
  kind?: string;
}

export type PickerPlacement = "bottom-start" | "bottom-end" | "top-start" | "top-end";

export declare const LES_PICKER_PARTS: readonly [
  "label",
  "combo",
  "input",
  "chip",
  "chip-remove",
  "adornment",
  "listbox",
  "option",
];

export type LesPickerPart = (typeof LES_PICKER_PARTS)[number];

/* Fired on selection change; bubbles and composed, so a listener on an
   ancestor outside the element sees it (§12.17). */
export interface LesPickerChangeEvent extends CustomEvent<{ selected: PickerItem[] }> {}

export declare class LesPicker extends HTMLElement {
  /* Attributes (§12.17): kebab-case in markup, camelCase here. */
  multiple: boolean;
  disabled: boolean;
  placeholder: string;
  label: string;
  minChars: number;
  debounce: number;
  /* `PickerPlacement` alone, not `| string`: the union collapses to `string`
     and checks nothing. An out-of-range value at runtime falls back to
     "bottom-start". */
  placement: PickerPlacement;
  offset: string;

  /* §12.19 — with a `name` the element is form-associated and submits one
     entry per selected value under that name; without one it does not
     participate and the consumer owns serialisation. */
  name: string;

  /* Properties carry data; children are data, not slots (§12.16). */
  items: PickerItem[] | null;
  source: ((query: string) => Promise<PickerItem[]>) | null;
  selected: PickerItem[];

  /* Reflected onto the host so a consumer can style on state that is not
     addressable from outside the element (§12.17). */
  open: boolean;

  readonly isRemote: boolean;
  readonly updateComplete: Promise<boolean>;

  addEventListener(
    type: "change",
    listener: (this: LesPicker, event: LesPickerChangeEvent) => void,
    options?: boolean | AddEventListenerOptions,
  ): void;
  addEventListener(
    type: string,
    listener: EventListenerOrEventListenerObject,
    options?: boolean | AddEventListenerOptions,
  ): void;
}

export declare const LES_BAR_PARTS: readonly [
  "burger",
  "app",
  "icon",
  "pages",
  "page",
  "current",
  "separator",
  "spacer",
  "account",
];

export type LesBarPart = (typeof LES_BAR_PARTS)[number];

export interface BarUser {
  name: string;
  portrait?: string;
}

/* The stand's header (R72). The pages are `<a>` CHILDREN read as data
   (§12.16), so they are in the initial HTML and still navigate if
   common-ui.js is blocked; mark the current one with `aria-current="page"`
   and the bar carries it through. At the theme's one breakpoint the pages
   collapse into the burger's popover — the THEME flips
   `--bar-pages-display`/`--bar-burger-display`, because a component writes no
   media query (§12.8). */
export declare class LesBar extends HTMLElement {
  /* Attribute. The short app name: "cron", "forms". Links to `/`. */
  app: string;
  /* Attribute. The app's emoji — the same one its registry tile carries,
     stamped from the shell's ONE `app_icon` marker, which also becomes the
     favicon. Shown after the name; empty means no chip and no gap. Marked
     aria-hidden, because the name already says which app this is. */
  icon: string;
  /* Attribute. The launcher root, supplied at RUNTIME from the config block
     because it differs per deployment class. R89(2): the app name links to
     this application's own root (`/`); the launcher is the account menu's
     FIRST entry, "All services", and is absent when this is empty. */
  launcher: string;
  /* The POST logout path, passed through to the account element. */
  logout: string;
  /* Property, injected from the config block's `user`. Without it no account
     element is rendered. */
  user: BarUser | null;
  /* Reflected: the burger's popover is open. */
  open: boolean;
}

export declare const LES_ACCOUNT_PARTS: readonly [
  "services",
  "trigger",
  "portrait",
  "initials",
  "name",
  "menu",
  "logout",
];

export type LesAccountPart = (typeof LES_ACCOUNT_PARTS)[number];

/* Rendered BY <les-bar>, and usable alone. Its parts are reachable through
   the bar as `account-<name>` (the bar sets `exportparts`), since a nested
   shadow root's parts are otherwise unreachable from the page. */
export declare class LesAccount extends HTMLElement {
  /* Attribute, required — it is what a service's "logged in as" chip shows. */
  name: string;
  /* Attribute. A URL; absent, or failing to load, falls back to initials
     derived from `name`. */
  portrait: string;
  /* Attribute. The POST path for R73's end-session route. A form submit, not
     a link: a link would issue GET and authentik would not end the session.
     Absent renders no logout control, so this element can ship before the
     route exists. */
  logout: string;
  /* Attribute. The launcher root; renders "All services" as the menu's first
     entry, absent when empty (R89). */
  launcher: string;
  /* Reflected: the menu is open. */
  open: boolean;
}

export declare const LES_TABLE_PARTS: readonly [
  "scroll",
  "table",
  "head",
  "header-cell",
  "sort",
  "body",
  "row",
  "cell",
  "badge",
  "label",
  "empty",
];

export type LesTablePart = (typeof LES_TABLE_PARTS)[number];

export type SortKind = "text" | "number" | "status" | "date";

/* Wraps a REAL `<table>` child, which it MOVES into its shadow root at
   upgrade. Moving preserves node identity, so cell content and any listener
   already attached survive — a clone would leave every action button in a
   cell dead, silently. Page code can no longer reach those nodes with
   `document.querySelector`; use ONE delegated listener on the host, since
   events from a shadow tree are composed.
 *
 * THE LAYOUT IS WHAT PLACES THE CELLS — it is not optional styling.
 * `--table-layout` is a `grid-template-areas` string whose names are the
 * column names (`data-col` on a `th`, defaulting to a slug of its text). A
 * multi-line value turns a row into a card; a name absent from the layout
 * hides that column. Set it in a media query and the layout switches with no
 * JS.
 *
 * With NO `--table-layout` the element builds a single-row layout from the
 * column names at upgrade, so an unstyled table is ugly rather than absent.
 * Before that default existed, an unstyled table rendered nothing at all: no
 * areas means every cell auto-places into an implicit track.
 *
 * NOT SUPPORTED: `colspan` and `rowspan` — grid placement ignores them, so a
 * spanning cell would land in the wrong place. Say it in the layout instead.
 *
 * `display: grid` drops the implicit table roles, so the element re-adds
 * `role=table/rowgroup/row/columnheader/cell` at upgrade. Unupgraded (a
 * blocked module) the child is a plain table with native semantics. */
export declare class LesTable extends HTMLElement {
  /* Attribute. Shown instead of an empty tbody. */
  empty: string;
}

export declare const LES_LINK_PARTS: readonly ["text", "copy", "open", "panel"];

export type LesLinkPart = (typeof LES_LINK_PARTS)[number];

/* A value you mostly want to COPY and occasionally want to follow. The light
 * DOM is an `<a href>` child: unupgraded it navigates and reads as a link,
 * which is why it is a child and not an attribute. After upgrade the text is
 * a BUTTON, because its job is to copy and a link that copies would lie to
 * the keyboard; a chain icon is the `<a>` that opens.
 *
 * The whole text copies. Ctrl/Cmd+click opens instead — the modifier a person
 * already uses to open a link elsewhere. A 1.2s "copied" state reflects on
 * the host and is announced through an aria-live region, because a
 * confirmation nobody asked for still has to reach a screen reader.
 *
 * CLIPBOARD NEEDS A SECURE CONTEXT. On a plain http origin
 * `navigator.clipboard` is undefined and the copy controls are NOT rendered —
 * a button that silently does nothing is worse than no button — while the
 * chain still opens. `http://127.0.0.1` IS a secure context. */
export declare class LesLink extends HTMLElement {
  /* Where the chain opens; taken from the `<a>` child when not set. */
  href: string;
  /* What lands on the clipboard, when that differs from the visible text. */
  copy: string;
  /* `_self` (default) or `_blank`. A new tab for an in-app link is the
     surprising one, so same-tab is the default. */
  target: string;
  /* Overrides the copy control's accessible name. */
  label: string;
  /* Reflected for 1.2s after a successful copy. */
  copied: boolean;
}

export declare const LES_HINT_PARTS: readonly ["trigger", "panel", "text"];

export type LesHintPart = (typeof LES_HINT_PARTS)[number];

/* Explanatory text the user ASKS for. The light DOM is the hint text, MOVED
 * into the panel at upgrade; unupgraded it renders as ordinary inline text,
 * which is still readable because the content IS the children. An optional
 * `[data-trigger]` child replaces the default ⓘ — moved, not cloned, so a
 * consumer's own icon and its listeners survive. `title` is NOT the
 * mechanism.
 *
 * Hover and focus open it only where the device has a pointer; a tap opens it
 * anywhere. Escape closes, and at most one popover is open across the whole
 * library because these are `auto` popovers. */
export declare class LesHint extends HTMLElement {
  /* The trigger's accessible name; defaults to "more information". */
  label: string;
  /* Reflected. */
  open: boolean;
}

export declare const LES_MODAL_PARTS: readonly [
  "dialog",
  "heading",
  "body",
  "actions",
  "close",
];

export type LesModalPart = (typeof LES_MODAL_PARTS)[number];

/* Fired when the dialog closes, however it closed — a `[data-close]` control,
   `close()`, Escape, or the close button. `returnValue` is what closed it. */
export interface LesModalCloseEvent extends CustomEvent<{ returnValue: string }> {}

/* A `<dialog>` opened with showModal(), so the top layer, the focus trap,
 * focus return and `inert` behind it are the platform's — all measured here,
 * all working, including across a shadow boundary.
 *
 * Two things are NOT the platform's and are done by the element: the
 * background scroll lock, which showModal() does not provide in this browser,
 * and swallowing the second half of one Escape when a popover open inside the
 * dialog has just consumed it (a picker in a dialog would otherwise take the
 * dialog down with its own menu).
 *
 * The light DOM is the body, MOVED in at upgrade; a `[data-actions]` child
 * becomes the footer; `[data-close="value"]` on any control inside closes
 * with that value, so the ordinary case needs no page code. Unupgraded, the
 * content is ordinary flow content — readable, and not pretending to be
 * modal.
 *
 * The backdrop takes `--modal-backdrop` and is NOT a part: a document-level
 * `::backdrop` rule does not reach into a shadow tree and `::backdrop` cannot
 * be named through `::part`. */
export declare class LesModal extends HTMLElement {
  /* Required — the dialog's accessible name. Without it the element refuses
     to open, because an unnamed dialog is announced as nothing. */
  heading: string;
  /* Reflected OUTPUT, and read-only in practice: setting the attribute or the
     property does NOT open the dialog. Call show() and close(), and listen
     for `close`. A framework that bound `open` as state got nothing at all —
     no dialog, and the host's own rect is 0x0 either way because the host is
     `display: contents`. */
  readonly open: boolean;
  show(): void;
  close(value?: string): void;
}

export declare const LES_CONFIRM_PARTS: readonly [
  "modal",
  "confirm",
  "cancel",
  "match",
  "match-input",
];

export type LesConfirmPart = (typeof LES_CONFIRM_PARTS)[number];

/* Wraps the control that performs the action — the real `<form>` or
 * `<button>` stays in the light DOM — so with the module blocked the action
 * still works, WITHOUT confirmation. That is the right way round: a guard
 * that exists only in JS would be removed by a broken bundle while the
 * destructive action survived.
 *
 * A form is re-submitted with requestSubmit(), so validation and the submit
 * event still happen; a bare button is re-activated with click(), so whatever
 * it was going to do remains its own business. */
export declare class LesConfirm extends HTMLElement {
  /* Required. The question, and the dialog's accessible name. */
  message: string;
  /* Attribute `confirm-label`. Default "Confirm". */
  confirmLabel: string;
  /* The confirm button takes the danger vocabulary. */
  danger: boolean;
  /* TYPE-TO-CONFIRM. When set, the dialog asks for this text and the confirm
     button stays disabled until the field matches it exactly — trimmed both
     sides, case-sensitive. Enter confirms when it matches and does nothing
     when it does not. Empty (the default) changes nothing. */
  match: string;
}

/* Fired after the user confirms and BEFORE the action proceeds. Cancelable:
   preventing it stops the action, because an event that cannot be prevented
   is a notification rather than a hook. */
export interface LesConfirmConfirmedEvent extends CustomEvent<undefined> {}

export declare const LES_TOAST_PARTS: readonly ["toast", "text", "dismiss"];

export type LesToastPart = (typeof LES_TOAST_PARTS)[number];

/* Transient status. Declarative first: a toast with text in its light DOM
 * shows itself at upgrade, which is this fleet's normal case — a redirect
 * after a POST. `show(text, kind)` is for the one case markup cannot serve, a
 * fetch result with no redirect.
 *
 * `role="status"` + `aria-live="polite"` for ok/info, `role="alert"` +
 * assertive for err. It never takes focus, and it is `popover="manual"` so
 * opening a menu does not dismiss it.
 *
 * It QUEUES while a modal dialog is open and raises on the dialog's close, in
 * order, with the visible duration starting then: a toast beside an open
 * modal is INERT and unreachable — modality, not stacking, so no z-index
 * fixes it — and a dead confirmation is worse than a late one. `err` queues
 * the same way, so an error that happens during a modal belongs in that
 * dialog's own actions. One queue per document, FIFO, no cap; one toast
 * visible at a time, so a `duration="0"` toast holds the queue until
 * dismissed.
 *
 * Structurally verified: role, aria-live, text, focus, and that the queue
 * drains in order. NOT verified: that a screen reader SPEAKS it. */
export declare class LesToast extends HTMLElement {
  /* `ok`, `info` or `err`. Reflected. Default `info`. */
  kind: string;
  /* Milliseconds; 0 keeps it until dismissed. Default 4000. */
  duration: number;
  show(text: string, kind?: string): void;
  hide(): void;
}

export declare const LES_TOOLBAR_PARTS: readonly [
  "heading",
  "actions",
  "panel",
  "overflow",
];

export type LesToolbarPart = (typeof LES_TOOLBAR_PARTS)[number];

/* The bar's behaviour one row down (R83, R84). `<a>` children are tabs when
 * any of them carries `aria-current`; `<button>` children are actions; a
 * `<button aria-pressed>` group is an in-page segmented switch.
 *
 * The children are MOVED once, so a listener a page attached to a button
 * still fires — unlike the bar, which re-renders its pages because they are
 * links with nothing attached. They are never moved again.
 *
 * At the theme breakpoint they collapse into ONE overflow popover
 * (`--toolbar-actions-display`, `--toolbar-overflow-display`), the bar's
 * mechanism exactly: a single copy that CSS turns back into a row while
 * closed, so nothing is in the tab order twice, nothing measures what fits,
 * and no component knows the breakpoint. The `aria-current` tab stays marked
 * inside the overflow. */
export declare class LesToolbar extends HTMLElement {
  /* Names the toolbar. NOT `title`, which is the tooltip attribute and would
     render a native tooltip over the whole row. */
  heading: string;
  /* Reflected while the overflow is open. */
  open: boolean;
}

export declare const LES_PANEL_PARTS: readonly [
  "panel",
  "header",
  "heading",
  "actions",
  "close",
  "body",
];

export type LesPanelPart = (typeof LES_PANEL_PARTS)[number];

/* A side panel (R94): fixed under the bar, full height, overlaying the page
 * rather than pushing it. NON-MODAL and it means it — no backdrop, no scroll
 * lock, no focus trap, and the page beneath stays scrollable and clickable.
 *
 * A plain fixed box rather than a dialog or a popover, on measurement: a
 * non-modal `dialog.show()` does not answer Escape at all, and a
 * `popover=auto` is closed both by another popover opening and by an outside
 * click — either of which would let the bar's own menu shut the panel.
 *
 * Escape closes it from anywhere UNLESS a modal or an open popover has
 * claimed the key, so a picker's menu inside the panel is dismissable without
 * taking the panel with it. One panel is open per document: opening one
 * closes any other.
 *
 * Children are MOVED into a scrollable body and moved back on close, so the
 * page's content survives being closed and reopened. A `[data-actions]` child
 * becomes the header's actions. `heading` is required and is the region's
 * accessible name. */
export declare class LesPanel extends HTMLElement {
  /* Required — the region's accessible name. Without it the element refuses
     to open, because an unnamed region announces nothing. */
  heading: string;
  /* Reflected OUTPUT, and read-only: setting it is REFUSED with a warning.
     It used to render the panel with an empty body and the children still in
     the light DOM, which looks like it worked. Call show() and close(). */
  readonly open: boolean;
  show(): void;
  close(): void;
}

/* The shared elements sheet as text, byte-identical to the linked
   elements.css (§12.11a). */
export declare const ELEMENTS_CSS: string;

export declare const STATUS: readonly [
  "ok",
  "pending",
  "running",
  "failed",
  "refused",
  "skipped",
  "disabled",
  "unknown",
];

/* §12.11 — a ninth status word is a type error, not an unstyled badge. */
export type Status = (typeof STATUS)[number];

declare global {
  interface HTMLElementTagNameMap {
    "les-picker": LesPicker;
    "les-bar": LesBar;
    "les-account": LesAccount;
    "les-table": LesTable;
    "les-hint": LesHint;
    "les-link": LesLink;
    "les-modal": LesModal;
    "les-confirm": LesConfirm;
    "les-toast": LesToast;
    "les-toolbar": LesToolbar;
    "les-panel": LesPanel;
  }
}
