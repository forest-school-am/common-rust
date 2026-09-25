var g=`@layer theme {
  /* Every selector here is a CLASS or a state. The SAME bytes are linked into
     the document after base.css (\xA712.14) and adopted inside each element's
     shadow root (\xA712.18), so a bare-tag reset such as \`button { border: 0 }\`
     would win over base.css's own buttons in the document copy. */

  /* base.css's \`* { box-sizing: border-box }\` is a DOCUMENT rule and rules do
     not cross a shadow boundary (\xA712.11a), so without this every element's
     internals are content-box: \`max-width: var(--field-max)\` measured 462px
     instead of 448 once padding and border were added. Identical to base.css's
     value, so the document copy of this sheet changes nothing there. */
  *,
  *::before,
  *::after {
    box-sizing: border-box;
  }

  /* The eight words of \xA712.11, and the distinction that earns three of them:
     refused never ran BY POLICY, skipped never ran BY SCHEDULE, failed ran and
     broke. A service maps its own states onto these rather than adding a
     ninth. The four neutral states are told apart by the word, not the colour
     \u2014 none of them means something went wrong.

     Here rather than in base.css because rules do not cross a shadow boundary
     and <les-table> badges its cells inside one; these bytes are linked into
     the document and adopted into every element, so there is ONE mapping. */
  .status-ok { color: var(--ok); }
  .status-pending { color: var(--mut); }
  .status-running { color: var(--acc); }
  .status-failed { color: var(--err); }
  .status-refused { color: var(--refused); }
  .status-skipped { color: var(--mut); }
  .status-disabled { color: var(--mut); }
  .status-unknown { color: var(--mut); }

  /* R86's footer: provenance, not chrome. One line that WRAPS on a phone
     rather than scrolling, muted, and held to the page measure so it lines up
     with the content above it. */
  footer {
    display: flex;
    flex-wrap: wrap;
    gap: var(--gap);
    /* Same trap as \`main\`: without an explicit width the auto margins absorb
       the free space of the flex column and the footer shrinks to its text. */
    width: 100%;
    max-width: var(--content-max);
    margin-inline: auto;
    padding: var(--pad-card);
    color: var(--mut, GrayText);
    border-top-style: solid;
    border-top-color: var(--bd, CanvasText);
    border-top-width: var(--bw);
  }
  /* THE FOOTER FOLLOWS MAIN'S MEASURE. Held to --content-max it lined up with
     nothing on a dashboard page, where \`main.wide\` spans the viewport: the
     rule under a 1252px-wide table stopped at 1152px. A footer is the page's
     own bottom edge, so it takes the page's own width. */
  body:has(main.wide) footer {
    max-width: none;
  }

  footer a {
    color: inherit;
  }

  /* R81's pill: one vocabulary for "a short labelled thing", absorbing the
     badge so there are not two that drift. Static on a span; interactive only
     when it IS a button or an anchor, which takes hover and focus from the
     button rules above rather than a role= hack. */
  /* ANCHORS, because page CSS cannot reach content this library MOVED into a
     shadow root \u2014 les-table appends the consumer's own <table> into #scroll,
     les-toolbar takes its tabs as children, and a panel and a modal move
     theirs. base.css says \`a { color: var(--acc) }\` and that rule stops at
     the boundary, so the UA colour took over: measured on cron-next's task
     links as UA blue 0000EE against theme-ink accent 9a4026, while the les-bar
     nav links on the same page were correctly themed. Invisible under
     theme-default alone, where the UA blue and the accent are both "blue".
     
     These bytes are adopted into every element (\xA712.11a), which is why the
     rule belongs here and not in each component's own sheet. \`a\` covers
     :visited too \u2014 an author rule beats the UA one, so there is no purple to
     undo and R101 gets no default link colours anywhere. */
  a {
    color: var(--acc, LinkText);
  }

  /* METERS AND FORM CONTROLS, the third thing page CSS cannot reach inside a
     cell this library moved. A consumer measured a <progress> in a table cell
     computing accent-color AUTO in both schemes, so its near-budget warning
     was invisible: a task at 95% of its disk budget drew the same grey bar as
     one at 5%. base.css themes a checkbox the same way and loses it at the
     same boundary \u2014 a row-selection box in a cell is the common case.
     
     accent-color: currentColor is what makes the EXISTING status vocabulary
     work here rather than a second one. The eight \xA712.11 words already set
     \`color\`, so \`<progress class="status-failed">\` paints its fill in --err
     with no new rule, no new token, and no list that can drift from the one
     above. \`color\` is set here too, so a bar with no status class is the
     accent rather than the body ink; a status class on the element wins it
     back by specificity. */
  progress {
    accent-color: currentColor;
    color: var(--acc, AccentColor);
    block-size: calc(var(--font-size) * 0.5);
    vertical-align: middle;
    /* THE TRACK, PAINTED. accent-color alone colours the FILL and leaves the
       track to the UA, which in dark mode is a solid bright white slab \u2014 the
       only light surface on the page, reading as "full" on a bar at 504 B of
       its budget, in all eleven themes. Light mode draws a modest outlined
       grey track, so nothing caught it until a dark review.
       
       Longhands, per the rule this build enforces: a border shorthand fed
       --bw is invalid as a whole when the palette is absent. */
    background: var(--card, Canvas);
    border-style: solid;
    border-color: var(--bd, ButtonBorder);
    border-width: var(--bw);
    border-radius: var(--roundness-pill);
  }

  /* The FILL. Firefox paints it through this pseudo, and currentColor is what
     keeps the eight status words working on it \u2014 the same mechanism as
     accent-color above, which stays for the controls that have no pseudo. */
  progress::-moz-progress-bar {
    background: currentColor;
    border-radius: var(--roundness-pill);
  }

  input[type="checkbox"],
  input[type="radio"] {
    accent-color: var(--acc, AccentColor);
    /* THE BOX ITSELF, not just the tick. In dark mode an unchecked control
       is the UA's own widget surface \u2014 a cool blue-grey square that ignores
       the palette, which is conspicuous on the warm grounds three themes
       use. accent-color paints the CHECKED state only. The resize grip on a
       textarea and the calendar glyph on a date input are drawn by the UA
       and cannot be reached without appearance:none, which would mean
       rebuilding both controls; they are left alone deliberately. */
    background-color: var(--card, Canvas);
  }

  /* WIDTH, in a CELL only. A bar left at the UA size is a fixed 162px, which
     made one consumer's disk column 240px wide against 94px for its
     neighbour, and no page rule could reach in to shrink it.
     
     Scoped to a cell rather than written on \`progress\`, because these bytes
     are ALSO the page's stylesheet: a blanket width here would resize every
     page-level bar in five services, none of which asked for it, and a bar
     on a page is one a page can already style.
     
     --progress-width is an instance knob with its fallback in the rule, not a
     palette token \u2014 a token would have to be declared by all eleven themes
     and would refuse every one of them until it was. Custom properties cross
     a shadow boundary, so a consumer sets it on the table, or on one column
     through ::part(cell-<column>), and gets any width it likes. */
  [part~="cell"] progress {
    inline-size: var(--progress-width, 100%);
  }

  /* MONO, for the same reason as the anchors above: base.css has this rule
     and base.css cannot reach content moved into a shadow root. A consumer
     put \`code, .mono\` in its page sheet, measured no pixel change, and found
     out the hard way \u2014 eighteen version strings and every cron expression
     rendering in the body font inside a table, with no way to fix it from
     outside. The fleet had no mono vocabulary that crossed the boundary; now
     the same two selectors work on both sides of it. */
  code,
  .mono {
    font-family: var(--font-mono);
    font-size: 0.88em;
  }

  .pill {
    display: inline-flex;
    align-items: center;
    gap: var(--gap);
    min-width: 0;
    /* SHRINK TO THE WORD. \`display: inline-flex\` is not enough when the pill
       is a GRID ITEM, which is what a table cell makes it \u2014 a grid item
       stretches to its track by default, so a status pill filled the whole
       cell: a 320px capsule with "ok" floating in the middle of it, and at a
       larger type size the longer words ran into the capsule edge. A pill
       states one word and should be exactly as wide as that word. */
    justify-self: start;
    padding: 0 var(--pad-card);
    border-radius: var(--roundness-pill);
    background: var(--chip, Canvas);
    color: var(--chipfg, CanvasText);
    font-weight: 600;
    white-space: nowrap;
  }
  .pill > .icon {
    flex: none;
    display: inline-flex;
  }
  /* The label may be long even though the pill is one line. */
  .pill > .label {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Intents: the wash grounds it and the role carries the text, which is the
     pairing the palette already asserts for AA. */
  .pill.primary {
    background: var(--acc-wash);
    color: var(--acc, CanvasText);
  }
  .pill.alt {
    background: var(--acc-wash);
    color: var(--alt, CanvasText);
  }
  .pill.ok,
  .pill.status-ok {
    background: var(--ok-wash);
    color: var(--ok, CanvasText);
  }
  .pill.warn,
  .pill.status-pending {
    background: var(--warn-wash);
    color: var(--warn, CanvasText);
  }
  .pill.danger,
  .pill.status-failed {
    background: var(--err-wash);
    color: var(--err, CanvasText);
  }
  .pill.status-refused {
    background: var(--refused-wash);
    color: var(--refused, CanvasText);
  }
  .pill.status-running {
    background: var(--acc-wash);
    color: var(--acc, CanvasText);
  }
  /* The four that mean nothing went wrong keep the neutral chip ground and
     are told apart by the WORD, as \xA712.11 has it. */
  .pill.status-skipped,
  .pill.status-disabled,
  .pill.status-unknown {
    background: var(--chip, Canvas);
    color: var(--mut, GrayText);
  }

  /* A pill that IS a control gets the touch floor and a pointer; a pill that
     is a span must not pretend to be either. */
  a.pill,
  button.pill {
    min-height: var(--control-min-height);
    cursor: pointer;
    text-decoration: none;
    border-style: none;
  }

  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    margin: -1px;
    padding: 0;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
    border: 0;
  }

  /* R80's button vocabulary: intent x emphasis x size x state, on native
     <button> and .btn. Bare words as ruled, but every rule is SCOPED to a
     button \u2014 a page's own \`.ok\` or \`.solid\` on something else does nothing,
     which is most of the collision risk bare words would otherwise carry.
     Specificity also keeps these BELOW .control-action, so a button inside a
     shared element (the picker's chip remove, the account trigger, the bar's
     burger) keeps its own shape. */
  .btn,
  button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--gap);
    min-height: var(--control-min-height);
    padding: 0 var(--pad-card);
    border-style: solid;
    border-width: var(--bw);
    border-color: var(--bd, CanvasText);
    border-radius: var(--roundness-btn);
    background: transparent;
    color: var(--fg, CanvasText);
    font-weight: var(--btn-weight, 600);
    cursor: pointer;
    text-decoration: none;
  }

  /* Intent: outlined is the ABSENCE of an emphasis class, and \`none\` the
     absence of an intent.
     
     PRIMARY IS THE EXCEPTION, and it is FILLED. It was outlined, with \`solid\`
     as the modifier that filled it \u2014 so the same theme gave one host a filled
     primary and two hosts outlined ones, and the page's one action weighed
     the same as everything beside it. R101 wants one look for it, and primary
     is the thing a page exists to do. \`primary solid\` is now a no-op rather
     than a different button. */
  .btn.primary,
  button.primary {
    background: var(--acc, LinkText);
    color: var(--acc-fg, Canvas);
    border-color: var(--acc, LinkText);
  }
  .btn.alt,
  button.alt {
    color: var(--alt, LinkText);
    border-color: var(--alt, LinkText);
  }
  .btn.ok,
  button.ok {
    color: var(--ok, CanvasText);
    border-color: var(--ok, CanvasText);
  }
  .btn.warn,
  button.warn {
    color: var(--warn, CanvasText);
    border-color: var(--warn, CanvasText);
  }
  .btn.danger,
  button.danger {
    color: var(--err, CanvasText);
    border-color: var(--err, CanvasText);
  }

  /* Solid. \`none\` + solid is the INVERTED neutral: --fg background with
     --card text, because --fg on --bd measures 4.30 in dark mode and fails
     AA, while this measures 13.96 and needs no new role. */
  .btn.solid,
  button.solid {
    background: var(--fg, CanvasText);
    color: var(--card, Canvas);
    border-color: transparent;
  }
  .btn.alt.solid,
  button.alt.solid {
    background: var(--alt, LinkText);
    color: var(--alt-fg, Canvas);
  }
  .btn.ok.solid,
  button.ok.solid {
    background: var(--ok, CanvasText);
    color: var(--ok-fg, Canvas);
  }
  .btn.warn.solid,
  button.warn.solid {
    background: var(--warn, CanvasText);
    color: var(--warn-fg, Canvas);
  }
  .btn.danger.solid,
  button.danger.solid {
    background: var(--err, CanvasText);
    color: var(--err-fg, Canvas);
  }

  .btn.quiet,
  button.quiet {
    border-color: transparent;
  }
  .btn.quiet:hover,
  button.quiet:hover {
    background: var(--row-hover);
  }

  /* \`sm\` is smaller TYPE and padding, never a smaller min-height: \xA712.9's
     touch floor is --control-min-height, the theme RAISES it on mobile, and a
     genuinely shorter button undoes exactly that on the device in the user's
     hand. */
  .btn.sm,
  button.sm {
    padding: 0 var(--pad-control);
    font-size: var(--btn-font-sm);
  }
  .btn.lg,
  button.lg {
    padding: 0 var(--gap-section);
    font-size: var(--btn-font-lg);
  }
  .btn.block,
  button.block {
    width: 100%;
  }

  /* No spinner: an animation nothing exercises is unverified (\xA712.2), and it
     can arrive with the first consumer that needs one. */
  .btn[aria-busy="true"],
  button[aria-busy="true"] {
    cursor: progress;
    opacity: 0.6;
    pointer-events: none;
  }

  .control {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--gap);
    min-height: var(--control-min-height);
    max-width: var(--field-max);
    padding: 0 var(--pad-control);
    /* Longhands for the same reason as the focus ring below: \`border:
       var(--bw) solid \u2026\` is invalid as a WHOLE when --bw is missing, and the
       control loses the only edge that says where it begins. As longhands the
       width falls back to \`medium\` and the edge survives. */
    border-style: solid;
    border-color: var(--bd, CanvasText);
    border-width: var(--bw);
    border-radius: var(--roundness-control);
    background: var(--card, Canvas);
    color: var(--fg, CanvasText);
    font: inherit;
  }
  .control:focus-within { border-color: var(--acc, AccentColor); }

  /* The field inside a control box owns no edge of its own \u2014 the box is the
     edge, and a second border would read as two nested controls. */
  .control-field {
    flex: 1 1 8rem;
    min-width: 8rem;
    min-height: var(--control-min-height);
    width: auto;
    max-width: none;
    padding: 0;
    border: 0;
    outline: 0;
    font: inherit;
    color: inherit;
    background: transparent;
  }

  /* L5: STATE CHANGES MOVE AT THE THEME'S PACE. Longhands, because a var()
     inside the transition shorthand is invalid as a whole when the token is
     missing. :where() so any component rule that sets its own transition
     still wins. At the 0s default nothing moves. */
  :where(.btn, button, .control, a.card, a.pill) {
    transition-property: background-color, border-color, color, box-shadow, translate;
    transition-duration: var(--motion-duration);
    transition-timing-function: var(--motion-easing);
  }

  /* L9: RAISED CONTROLS, WELLS AND THE PRESS. A button or a control pill is
     raised by --shadow-control and pressed to --shadow-press, moving by
     --press-translate while the press lasts, whether the press is a pointer
     (:active) or a toggle ([aria-pressed="true"]). A quiet button and the
     small actions inside a control box (the picker's clear, the burger) stay
     flat: a raised chip inside a field reads as a second control. The control
     box itself is a field and takes --shadow-field. :where() keeps all of it at
     class specificity, so a page rule can still restyle a button. */
  :where(.btn, button, a.pill, button.pill):not(.quiet, .control-action) {
    box-shadow: var(--shadow-control);
  }
  :where(.btn, button, a.pill, button.pill):not(.quiet, .control-action):is(:active, [aria-pressed="true"]) {
    box-shadow: var(--shadow-press);
    translate: var(--press-translate);
  }
  .control {
    box-shadow: var(--shadow-field);
  }

  /* L8: STATE LAYERS. A veil of the element's own ink over whatever fills
     it, drawn as a background-image so the fill underneath is untouched and
     an outlined button, a filled primary and a pill all get the same cue. At
     the 0% default the veil is fully transparent. A quiet button keeps
     --row-hover, which is its own hover language. */
  :where(button, .btn, a.pill, .control-action, [part~="page"]):not(.quiet):hover {
    background-image: linear-gradient(
      color-mix(in srgb, currentColor var(--state-hover), transparent),
      color-mix(in srgb, currentColor var(--state-hover), transparent)
    );
  }
  :where(button, .btn, a.pill, .control-action, [part~="page"]):not(.quiet):active {
    background-image: linear-gradient(
      color-mix(in srgb, currentColor var(--state-press), transparent),
      color-mix(in srgb, currentColor var(--state-press), transparent)
    );
  }

  /* Reduced motion is the USER's, so it wins over every theme: a property
     rule here reaches every shadow root that adopts this sheet, where a token
     override would lose to the unlayered palette. */
  @media (prefers-reduced-motion: reduce) {
    :where(.btn, button, .control, a.card, a.pill,
      [part~="panel"], [part~="listbox"], [part~="dialog"], [part~="toast"]) {
      transition-duration: 0s;
    }
  }

  /* The theme's ring on every focusable these sheets style. Only the control
     vocabulary had it, so a button, a link or a bare field showed the UA's
     ring whatever the theme's --ring said. Before the control rule below,
     which has the same specificity and must keep its inset ring. */
  :where(button, .btn, a, input, select, textarea):focus-visible {
    outline-style: solid;
    outline-color: var(--ring, AccentColor);
    outline-width: var(--ring-width);
    outline-offset: 2px;
  }

  /* Longhands, not the \`outline\` shorthand: with no palette linked
     \`var(--ring-width)\` is invalid at computed-value time and takes the WHOLE
     shorthand with it, so the focus ring disappears entirely. As longhands
     only the width falls back \u2014 to \`medium\` \u2014 and the ring stays visible,
     which is the one thing that must survive a palette that failed to load. */
  :where(.control, .control-field, .control-action):focus-visible {
    outline-style: solid;
    outline-color: var(--ring, AccentColor);
    outline-width: var(--ring-width);
    outline-offset: calc(-1 * var(--ring-width, 2px));
  }

  .control-action {
    min-height: var(--control-min-height);
    font: inherit;
    color: inherit;
    background: transparent;
    border: 0;
    border-radius: var(--roundness-control);
    cursor: pointer;
  }

  :where(.control, .control-field, .control-action)[disabled],
  :where(.control, .control-field, .control-action):disabled {
    opacity: 0.6;
    cursor: default;
  }
  .control[disabled] { pointer-events: none; }

  /* ---------- R87: one trigger, no element ---------- */

  /* The CARRIER LIST is in the selector, not in prose. Buttons keep the R80
     treatment above and get no spinner (R87); a link and the form controls
     are excluded because the overlay is for a REGION. \`input\` is the measured
     case: it generates the pseudo with ZERO width, so the overlay would be
     invisible while the min-height still stretched the field \u2014 my first draft
     made a busy button 96px tall, which is what put the list here.
     
     [data-busy-internal] is how an element says it draws its OWN overlay. A
     host's pseudo paints beneath its shadow content, so a busy <les-table>
     showed the host's ring THROUGH the gaps in its own content as well as the
     one inside it \u2014 a pixel scan counted 238 ring pixels against a page
     region's 126. The element sets this attribute on itself, which leaves
     exactly one overlay and puts the min-height and pointer-events on the
     wrapper that holds the content. */
  :where([aria-busy="true"]):not(
      button, a, input, select, textarea, [role="button"], [role="link"],
      [data-busy-internal]
    ) {
    position: relative;
    min-height: var(--busy-min-height);
  }

  /* Left in flow and unreachable, so the content stays measurable and a real
     click cannot land on it. */
  :where([aria-busy="true"]):not(
      button, a, input, select, textarea, [role="button"], [role="link"],
      [data-busy-internal]
    ) > * {
    pointer-events: none;
  }

  /* TWO pseudo-elements, because only the RING may rotate. With the ring as
     a background image of the scrim layer, animating transform turned the
     whole overlay \u2014 the dark rectangle and the label with it \u2014 which is both
     wrong to look at and why a pixel probe could not find the arc where the
     geometry said it was. ::before is the ring, ::after is the scrim and the
     label, and z-index puts the ring above the scrim since ::before paints
     first. */
  :where([aria-busy="true"]):not(
      button, a, input, select, textarea, [role="button"], [role="link"],
      [data-busy-internal]
    )::before {
    content: "";
    position: absolute;
    z-index: 3;
    /* CENTRED ON THE CARRIER, ring alone. */
    inset-block-start: calc(50% - var(--spinner-size) / 2);
    inset-inline-start: calc(50% - var(--spinner-size) / 2);
    width: var(--spinner-size);
    height: var(--spinner-size);
    /* A BORDER ring, not a conic gradient. The gradient interpolated from
       transparent to the colour, so most of the arc was semi-transparent and
       composited over whatever was beneath it \u2014 two overlays over different
       content could not be compared, and "same look" could not be measured.
       A border arc is OPAQUE where it paints.
       
       The pill token is the library's "fully round"; a literal 50% is a
       design constant (\xA712.13) and the box is square anyway. */
    border-radius: var(--roundness-pill);
    border-style: solid;
    border-width: var(--spinner-width);
    border-color: transparent;
    border-block-start-color: var(--spinner-color);
    /* A theme may replace the ring glyph entirely; a data: URI mask is
       accepted under the crate's CSP (measured \u2014 img-src governs CSS images
       too, which is what the favicon taught us). */
    mask-image: var(--spinner-image);
    mask-repeat: no-repeat;
    mask-position: center;
    mask-size: contain;

    /* TWO animations: the ring, and a one-shot fade that keeps it invisible
       for ~200ms so a fast fetch shows nothing.
       
       A transition cannot do this: the pseudo-element is CREATED when the
       attribute appears, so there is no previous opacity to transition FROM
       and it paints at its final value \u2014 measured at opacity 1 after 80ms
       with a 200ms transition delay in place. */
    opacity: 0;
    animation:
      var(--spinner-animation) var(--spinner-duration) linear infinite,
      busy-in 0.12s linear 0.2s forwards;
  }

  /* WITH A LABEL, the PAIR is centred, not the ring. The label is one line
     below the ring plus a gap, so the ring moves up by half that block and
     the label centring in the padded box covers the other half.
     
     Before this rule the ring was placed a FULL spinner above centre whether
     or not there was a label, which is fine in a tall carrier and wrong in a
     short one: in the gallery's two-row busy example \u2014 a 95px carrier \u2014 the
     ring sat ON the carrier top edge, clipped by it and overlapping the data
     row, and a reviewer read it as the spinner straddling the boundary. It
     was, and the arithmetic said so. */
  :where([aria-busy="true"][data-busy-text]):not(
      button, a, input, select, textarea, [role="button"], [role="link"],
      [data-busy-internal]
    )::before {
    inset-block-start: calc(50% - var(--spinner-size) / 2 - (var(--gap) + 1lh) / 2);
  }

  :where([aria-busy="true"]):not(
      button, a, input, select, textarea, [role="button"], [role="link"],
      [data-busy-internal]
    )::after {
    content: attr(data-busy-text);
    position: absolute;
    inset: 0;
    z-index: 2;
    display: flex;
    align-items: center;
    justify-content: center;
    /* The label sits UNDER the ring: the ring is shifted up by half of this
       reserved block and the label is centred in what is left, so the PAIR is
       centred on the carrier at any height. See the ::before rule. */
    padding-block-start: calc(var(--spinner-size) + var(--gap));
    /* A VEIL OF THE SURFACE, not the modal's backdrop. It borrowed
       --modal-backdrop, which is a dark curtain by design \u2014 the thing a
       DIALOG sits in front of \u2014 and in light mode that made a loading table
       the darkest thing on the page: it read as disabled rather than busy.
       A translucent card veils the content in either scheme without blacking
       it out, and it needs no palette token of its own because the surface
       already knows what colour it is. */
    background-color: color-mix(in srgb, var(--card, Canvas) 82%, transparent);
    color: var(--fg, CanvasText);
    text-align: center;
    opacity: 0;
    animation: busy-in 0.12s linear 0.2s forwards;
  }

  /* HERE, not in the palette: keyframes resolve in the tree scope of the
     animating element, so a name defined in a page sheet does not reach an
     element's shadow root \u2014 0 running animations there against 1 on the page.
     These bytes are adopted into every shadow root, so \`spin\` resolves in
     both. A theme naming its own animation reaches page markup only. */
  @keyframes spin {
    to { transform: rotate(1turn); }
  }

  @keyframes busy-in {
    to { opacity: 1; }
  }

  @keyframes busy-pulse {
    50% { opacity: 0.45; }
  }

  @media (prefers-reduced-motion: reduce) {
    /* BOTH names: animation-name is a list, and naming only the pulse drops
       the fade and takes the 200ms delay with it. On ::before, which is where
       the ring's animation lives. */
    :where([aria-busy="true"]):not(
        button, a, input, select, textarea, [role="button"], [role="link"],
        [data-busy-internal]
      )::before {
      animation-name: busy-pulse, busy-in;
    }
  }

  /* ---------- R83: the card, a class and pure grouping ---------- */

  /* Children named by CLASS take the grid-area of that name, so the layout is
     a parameter (--card-layout) exactly as the table's is, and a phone gets a
     different arrangement from a media query with no JS. */
  .card {
    display: grid;
    grid-template-areas: var(--card-layout);
    grid-template-columns: var(--card-columns);
    gap: var(--gap) var(--gap-section);
    padding: var(--pad-card);
    border-style: solid;
    border-width: var(--bw-card);
    border-color: var(--bd-surface, var(--bd, ButtonBorder));
    border-radius: var(--roundness-card);
    background: var(--surface-1, Canvas);
    box-shadow: var(--shadow-card);
    color: var(--fg, CanvasText);
    /* The glass layer below is positioned against the card and stacked inside it. */
    position: relative;
    isolation: isolate;
    /* L17: with no backdrop-filter a translucent step would be see-through with no frost, so the opaque card instead. */
    @supports not (backdrop-filter: blur(1px)) {
      background: var(--card, Canvas);
    }
  }

  /* L17 .SURFACE: a panel that PAINTS like a card and does nothing to layout. No display, grid, padding,
     margin or gap, so a consumer's own panel keeps its box and only its paint changes: in every theme that is
     not glass it computes the same background, border, radius and shadow as .card, and in a glass theme it is
     frosted glass like one. position and isolation are there for the glass layer: like a card, a surface is a
     stacking context, so something fixed inside it is stacked inside it (les-panel is in the top layer and
     escapes it). */
  .surface {
    border-style: solid;
    border-width: var(--bw-card);
    border-color: var(--bd-surface, var(--bd, ButtonBorder));
    border-radius: var(--roundness-card);
    background: var(--surface-1, Canvas);
    box-shadow: var(--shadow-card);
    position: relative;
    isolation: isolate;
    /* L17: with no backdrop-filter a translucent step would be see-through with no frost, so the opaque card instead. */
    @supports not (backdrop-filter: blur(1px)) {
      background: var(--card, Canvas);
    }
  }

  /* L17 GLASS LAYER: the frost is a backdrop-filter on ::before, never on the host, so the host never becomes the containing block for anything fixed inside it. The layer has no background: it blurs the host's own translucent background over what lies behind.
     A busy card or surface leaves ::before to the busy ring and keeps its tint under the scrim. */
  .card:not([aria-busy="true"])::before,
  .surface:not([aria-busy="true"])::before {
    content: "";
    position: absolute;
    inset: 0;
    z-index: -1;
    border-radius: inherit;
    pointer-events: none;
    backdrop-filter: var(--surface-blur);
  }

  .card > .title { grid-area: title; font-weight: 600; }
  .card > .actions {
    grid-area: actions;
    display: flex;
    flex-wrap: wrap;
    gap: var(--gap);
    align-items: center;
    justify-content: flex-end;
  }
  .card > .body { grid-area: body; }
  .card > .footer { grid-area: footer; color: var(--mut, GrayText); }
  .card > .media { grid-area: media; }

  /* Intent and emphasis, mirroring R80's buttons: absence of an emphasis
     class is bordered, absence of an intent is neutral. Scoped to .card for
     the same reason the button rules are scoped to a button \u2014 a page's own
     \`.ok\` on something else does nothing. */
  .card.primary { border-color: var(--acc, LinkText); }
  .card.alt { border-color: var(--alt, LinkText); }
  .card.ok { border-color: var(--ok, CanvasText); }
  .card.warn { border-color: var(--warn, CanvasText); }
  .card.danger { border-color: var(--err, CanvasText); }
  /* L16: the intent's colour on a stripe of its own width, so a borderless
     card (--bw-surface: 0) keeps the cue on its leading edge. */
  .card:is(.primary, .alt, .ok, .warn, .danger) { border-inline-start-width: var(--bw-card-intent); }

  /* Solid is a WASH, not an inversion: a card is a surface holding text, and
     --acc-fg on --acc is right for a button's label and wrong for a
     paragraph. */
  .card.solid { background: color-mix(in srgb, var(--fg, CanvasText) 6%, var(--card, Canvas)); }
  .card.primary.solid { background: color-mix(in srgb, var(--acc, LinkText) 10%, var(--card, Canvas)); }
  .card.alt.solid { background: color-mix(in srgb, var(--alt, LinkText) 10%, var(--card, Canvas)); }
  .card.ok.solid { background: color-mix(in srgb, var(--ok, CanvasText) 10%, var(--card, Canvas)); }
  .card.warn.solid { background: color-mix(in srgb, var(--warn, CanvasText) 10%, var(--card, Canvas)); }
  .card.danger.solid { background: color-mix(in srgb, var(--err, CanvasText) 10%, var(--card, Canvas)); }

  .card.quiet { border-color: transparent; background: none; }

  /* The link case. */
  a.card { text-decoration: none; }
  a.card:hover { border-color: var(--acc, LinkText); translate: var(--hover-lift); }

  /* The card grid. --col-min is the theme's, so the tile count follows the
     viewport with no breakpoint of its own. */
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(var(--col-min), 1fr));
    gap: var(--gap-section);
  }
}
`;/**
 * @license
 * Copyright 2019 Google LLC
 * SPDX-License-Identifier: BSD-3-Clause
 */var K=globalThis,fe=K.ShadowRoot&&(K.ShadyCSS===void 0||K.ShadyCSS.nativeShadow)&&"adoptedStyleSheets"in Document.prototype&&"replace"in CSSStyleSheet.prototype,ve=Symbol(),Ee=new WeakMap,G=class{constructor(e,t,r){if(this._$cssResult$=!0,r!==ve)throw Error("CSSResult is not constructable. Use `unsafeCSS` or `css` instead.");this.cssText=e,this.t=t}get styleSheet(){let e=this.i,t=this.t;if(fe&&e===void 0){let r=t!==void 0&&t.length===1;r&&(e=Ee.get(t)),e===void 0&&((this.i=e=new CSSStyleSheet).replaceSync(this.cssText),r&&Ee.set(t,e))}return e}toString(){return this.cssText}},m=a=>new G(typeof a=="string"?a:a+"",void 0,ve),p=(a,...e)=>{let t=a.length===1?a[0]:e.reduce((r,o,n)=>r+(s=>{if(s._$cssResult$===!0)return s.cssText;if(typeof s=="number")return s;throw Error("Value passed to 'css' function must be a 'css' function result: "+s+". Use 'unsafeCSS' to pass non-literal values, but take care to ensure page security.")})(o)+a[n+1],a[0]);return new G(t,a,ve)},Ke=(a,e)=>{if(fe)a.adoptedStyleSheets=e.map(t=>t instanceof CSSStyleSheet?t:t.styleSheet);else for(let t of e){let r=document.createElement("style"),o=K.litNonce;o!==void 0&&r.setAttribute("nonce",o),r.textContent=t.cssText,a.appendChild(r)}},Ae=fe?a=>a:a=>a instanceof CSSStyleSheet?(e=>{let t="";for(let r of e.cssRules)t+=r.cssText;return m(t)})(a):a,{is:Ge,defineProperty:je,getOwnPropertyDescriptor:Ve,getOwnPropertyNames:Ye,getOwnPropertySymbols:Xe,getPrototypeOf:Je}=Object,V=globalThis,Ce=V.trustedTypes,Ze=Ce?Ce.emptyScript:"",Qe=V.reactiveElementPolyfillSupport,H=(a,e)=>a,ce={toAttribute(a,e){switch(e){case Boolean:a=a?Ze:null;break;case Object:case Array:a=a==null?a:JSON.stringify(a)}return a},fromAttribute(a,e){let t=a;switch(e){case Boolean:t=a!==null;break;case Number:t=a===null?null:Number(a);break;case Object:case Array:try{t=JSON.parse(a)}catch{t=null}}return t}},He=(a,e)=>!Ge(a,e),_e={attribute:!0,type:String,converter:ce,reflect:!1,useDefault:!1,hasChanged:He};Symbol.metadata??=Symbol("metadata"),V.litPropertyMetadata??=new WeakMap;var x=class extends HTMLElement{static addInitializer(e){this.o(),(this.l??=[]).push(e)}static get observedAttributes(){return this.finalize(),this.u&&[...this.u.keys()]}static createProperty(e,t=_e){if(t.state&&(t.attribute=!1),this.o(),this.prototype.hasOwnProperty(e)&&((t=Object.create(t)).wrapped=!0),this.elementProperties.set(e,t),!t.noAccessor){let r=Symbol(),o=this.getPropertyDescriptor(e,r,t);o!==void 0&&je(this.prototype,e,o)}}static getPropertyDescriptor(e,t,r){let{get:o,set:n}=Ve(this.prototype,e)??{get(){return this[t]},set(s){this[t]=s}};return{get:o,set(s){let h=o?.call(this);n?.call(this,s),this.requestUpdate(e,h,r)},configurable:!0,enumerable:!0}}static getPropertyOptions(e){return this.elementProperties.get(e)??_e}static o(){if(this.hasOwnProperty(H("elementProperties")))return;let e=Je(this);e.finalize(),e.l!==void 0&&(this.l=[...e.l]),this.elementProperties=new Map(e.elementProperties)}static finalize(){if(this.hasOwnProperty(H("finalized")))return;if(this.finalized=!0,this.o(),this.hasOwnProperty(H("properties"))){let t=this.properties,r=[...Ye(t),...Xe(t)];for(let o of r)this.createProperty(o,t[o])}let e=this[Symbol.metadata];if(e!==null){let t=litPropertyMetadata.get(e);if(t!==void 0)for(let[r,o]of t)this.elementProperties.set(r,o)}this.u=new Map;for(let[t,r]of this.elementProperties){let o=this.p(t,r);o!==void 0&&this.u.set(o,t)}this.elementStyles=this.finalizeStyles(this.styles)}static finalizeStyles(e){let t=[];if(Array.isArray(e)){let r=new Set(e.flat(1/0).reverse());for(let o of r)t.unshift(Ae(o))}else e!==void 0&&t.push(Ae(e));return t}static p(e,t){let r=t.attribute;return r===!1?void 0:typeof r=="string"?r:typeof e=="string"?e.toLowerCase():void 0}constructor(){super(),this.v=void 0,this.isUpdatePending=!1,this.hasUpdated=!1,this.m=null,this._()}_(){this.S=new Promise(e=>this.enableUpdating=e),this._$AL=new Map,this.$(),this.requestUpdate(),this.constructor.l?.forEach(e=>e(this))}addController(e){(this.P??=new Set).add(e),this.renderRoot!==void 0&&this.isConnected&&e.hostConnected?.()}removeController(e){this.P?.delete(e)}$(){let e=new Map,t=this.constructor.elementProperties;for(let r of t.keys())this.hasOwnProperty(r)&&(e.set(r,this[r]),delete this[r]);e.size>0&&(this.v=e)}createRenderRoot(){let e=this.shadowRoot??this.attachShadow(this.constructor.shadowRootOptions);return Ke(e,this.constructor.elementStyles),e}connectedCallback(){this.renderRoot??=this.createRenderRoot(),this.enableUpdating(!0),this.P?.forEach(e=>e.hostConnected?.())}enableUpdating(e){}disconnectedCallback(){this.P?.forEach(e=>e.hostDisconnected?.())}attributeChangedCallback(e,t,r){this._$AK(e,r)}C(e,t){let r=this.constructor.elementProperties.get(e),o=this.constructor.p(e,r);if(o!==void 0&&r.reflect===!0){let n=(r.converter?.toAttribute!==void 0?r.converter:ce).toAttribute(t,r.type);this.m=e,n==null?this.removeAttribute(o):this.setAttribute(o,n),this.m=null}}_$AK(e,t){let r=this.constructor,o=r.u.get(e);if(o!==void 0&&this.m!==o){let n=r.getPropertyOptions(o),s=typeof n.converter=="function"?{fromAttribute:n.converter}:n.converter?.fromAttribute!==void 0?n.converter:ce;this.m=o;let h=s.fromAttribute(t,n.type);this[o]=h??this.T?.get(o)??h,this.m=null}}requestUpdate(e,t,r,o=!1,n){if(e!==void 0){let s=this.constructor;if(o===!1&&(n=this[e]),r??=s.getPropertyOptions(e),!((r.hasChanged??He)(n,t)||r.useDefault&&r.reflect&&n===this.T?.get(e)&&!this.hasAttribute(s.p(e,r))))return;this.M(e,t,r)}this.isUpdatePending===!1&&(this.S=this.k())}M(e,t,{useDefault:r,reflect:o,wrapped:n},s){r&&!(this.T??=new Map).has(e)&&(this.T.set(e,s??t??this[e]),n!==!0||s!==void 0)||(this._$AL.has(e)||(this.hasUpdated||r||(t=void 0),this._$AL.set(e,t)),o===!0&&this.m!==e&&(this.A??=new Set).add(e))}async k(){this.isUpdatePending=!0;try{await this.S}catch(t){Promise.reject(t)}let e=this.scheduleUpdate();return e!=null&&await e,!this.isUpdatePending}scheduleUpdate(){return this.performUpdate()}performUpdate(){if(!this.isUpdatePending)return;if(!this.hasUpdated){if(this.renderRoot??=this.createRenderRoot(),this.v){for(let[o,n]of this.v)this[o]=n;this.v=void 0}let r=this.constructor.elementProperties;if(r.size>0)for(let[o,n]of r){let{wrapped:s}=n,h=this[o];s!==!0||this._$AL.has(o)||h===void 0||this.M(o,void 0,n,h)}}let e=!1,t=this._$AL;try{e=this.shouldUpdate(t),e?(this.willUpdate(t),this.P?.forEach(r=>r.hostUpdate?.()),this.update(t)):this.U()}catch(r){throw e=!1,this.U(),r}e&&this._$AE(t)}willUpdate(e){}_$AE(e){this.P?.forEach(t=>t.hostUpdated?.()),this.hasUpdated||(this.hasUpdated=!0,this.firstUpdated(e)),this.updated(e)}U(){this._$AL=new Map,this.isUpdatePending=!1}get updateComplete(){return this.getUpdateComplete()}getUpdateComplete(){return this.S}shouldUpdate(e){return!0}update(e){this.A&&=this.A.forEach(t=>this.C(t,this[t])),this.U()}updated(e){}firstUpdated(e){}};x.elementStyles=[],x.shadowRootOptions={mode:"open"},x[H("elementProperties")]=new Map,x[H("finalized")]=new Map,Qe?.({ReactiveElement:x}),(V.reactiveElementVersions??=[]).push("2.1.2");/**
 * @license
 * Copyright 2017 Google LLC
 * SPDX-License-Identifier: BSD-3-Clause
 */var we=globalThis,$e=a=>a,j=we.trustedTypes,Le=j?j.createPolicy("lit-html",{createHTML:a=>a}):void 0,Ne="$lit$",S=`lit$${Math.random().toFixed(9).slice(2)}$`,Ue="?"+S,et=`<${Ue}>`,_=document,N=()=>_.createComment(""),U=a=>a===null||typeof a!="object"&&typeof a!="function",ye=Array.isArray,tt=a=>ye(a)||typeof a?.[Symbol.iterator]=="function",de=`[ 	
\f\r]`,M=/<(?:(!--|\/[^a-zA-Z])|(\/?[a-zA-Z][^>\s]*)|(\/?$))/g,Re=/-->/g,Pe=/>/g,A=RegExp(`>|${de}(?:([^\\s"'>=/]+)(${de}*=${de}*(?:[^ 	
\f\r"'\`<>=]|("|')|))|$)`,"g"),Oe=/'/g,Ie=/"/g,qe=/^(?:script|style|textarea|title)$/i,ke=a=>(e,...t)=>({_$litType$:a,strings:e,values:t}),d=ke(1),Mt=ke(2),Ht=ke(3),$=Symbol.for("lit-noChange"),i=Symbol.for("lit-nothing"),Me=new WeakMap,C=_.createTreeWalker(_,129);function Be(a,e){if(!ye(a)||!a.hasOwnProperty("raw"))throw Error("invalid template strings array");return Le!==void 0?Le.createHTML(e):e}var rt=(a,e)=>{let t=a.length-1,r=[],o,n=e===2?"<svg>":e===3?"<math>":"",s=M;for(let h=0;h<t;h++){let l=a[h],u,f,b=-1,v=0;for(;v<l.length&&(s.lastIndex=v,f=s.exec(l),f!==null);)v=s.lastIndex,s===M?f[1]==="!--"?s=Re:f[1]!==void 0?s=Pe:f[2]!==void 0?(qe.test(f[2])&&(o=RegExp("</"+f[2],"g")),s=A):f[3]!==void 0&&(s=A):s===A?f[0]===">"?(s=o??M,b=-1):f[1]===void 0?b=-2:(b=s.lastIndex-f[2].length,u=f[1],s=f[3]===void 0?A:f[3]==='"'?Ie:Oe):s===Ie||s===Oe?s=A:s===Re||s===Pe?s=M:(s=A,o=void 0);let k=s===A&&a[h+1].startsWith("/>")?" ":"";n+=s===M?l+et:b>=0?(r.push(u),l.slice(0,b)+Ne+l.slice(b)+S+k):l+S+(b===-2?h:k)}return[Be(a,n+(a[t]||"<?>")+(e===2?"</svg>":e===3?"</math>":"")),r]},q=class a{constructor({strings:e,_$litType$:t},r){let o;this.parts=[];let n=0,s=0,h=e.length-1,l=this.parts,[u,f]=rt(e,t);if(this.el=a.createElement(u,r),C.currentNode=this.el.content,t===2||t===3){let b=this.el.content.firstChild;b.replaceWith(...b.childNodes)}for(;(o=C.nextNode())!==null&&l.length<h;){if(o.nodeType===1){if(o.hasAttributes())for(let b of o.getAttributeNames())if(b.endsWith(Ne)){let v=f[s++],k=o.getAttribute(b).split(S),F=/([.?@])?(.*)/.exec(v);l.push({type:1,index:n,name:F[2],strings:k,ctor:F[1]==="."?ue:F[1]==="?"?be:F[1]==="@"?ge:R}),o.removeAttribute(b)}else b.startsWith(S)&&(l.push({type:6,index:n}),o.removeAttribute(b));if(qe.test(o.tagName)){let b=o.textContent.split(S),v=b.length-1;if(v>0){o.textContent=j?j.emptyScript:"";for(let k=0;k<v;k++)o.append(b[k],N()),C.nextNode(),l.push({type:2,index:++n});o.append(b[v],N())}}}else if(o.nodeType===8)if(o.data===Ue)l.push({type:2,index:n});else{let b=-1;for(;(b=o.data.indexOf(S,b+1))!==-1;)l.push({type:7,index:n}),b+=S.length-1}n++}}static createElement(e,t){let r=_.createElement("template");return r.innerHTML=e,r}};function L(a,e,t=a,r){if(e===$)return e;let o=r!==void 0?t.N?.[r]:t.O,n=U(e)?void 0:e._$litDirective$;return o?.constructor!==n&&(o?._$AO?.(!1),n===void 0?o=void 0:(o=new n(a),o._$AT(a,t,r)),r!==void 0?(t.N??=[])[r]=o:t.O=o),o!==void 0&&(e=L(a,o._$AS(a,e.values),o,r)),e}var pe=class{constructor(e,t){this._$AV=[],this._$AN=void 0,this._$AD=e,this._$AM=t}get parentNode(){return this._$AM.parentNode}get _$AU(){return this._$AM._$AU}R(e){let{el:{content:t},parts:r}=this._$AD,o=(e?.creationScope??_).importNode(t,!0);C.currentNode=o;let n=C.nextNode(),s=0,h=0,l=r[0];for(;l!==void 0;){if(s===l.index){let u;l.type===2?u=new B(n,n.nextSibling,this,e):l.type===1?u=new l.ctor(n,l.name,l.strings,this,e):l.type===6&&(u=new me(n,this,e)),this._$AV.push(u),l=r[++h]}s!==l?.index&&(n=C.nextNode(),s++)}return C.currentNode=_,o}V(e){let t=0;for(let r of this._$AV)r!==void 0&&(r.strings!==void 0?(r._$AI(e,r,t),t+=r.strings.length-2):r._$AI(e[t])),t++}},B=class a{get _$AU(){return this._$AM?._$AU??this.D}constructor(e,t,r,o){this.type=2,this._$AH=i,this._$AN=void 0,this._$AA=e,this._$AB=t,this._$AM=r,this.options=o,this.D=o?.isConnected??!0}get parentNode(){let e=this._$AA.parentNode,t=this._$AM;return t!==void 0&&e?.nodeType===11&&(e=t.parentNode),e}get startNode(){return this._$AA}get endNode(){return this._$AB}_$AI(e,t=this){e=L(this,e,t),U(e)?e===i||e==null||e===""?(this._$AH!==i&&this._$AR(),this._$AH=i):e!==this._$AH&&e!==$&&this.L(e):e._$litType$!==void 0?this.j(e):e.nodeType!==void 0?this.I(e):tt(e)?this.H(e):this.L(e)}B(e){return this._$AA.parentNode.insertBefore(e,this._$AB)}I(e){this._$AH!==e&&(this._$AR(),this._$AH=this.B(e))}L(e){this._$AH!==i&&U(this._$AH)?this._$AA.nextSibling.data=e:this.I(_.createTextNode(e)),this._$AH=e}j(e){let{values:t,_$litType$:r}=e,o=typeof r=="number"?this._$AC(e):(r.el===void 0&&(r.el=q.createElement(Be(r.h,r.h[0]),this.options)),r);if(this._$AH?._$AD===o)this._$AH.V(t);else{let n=new pe(o,this),s=n.R(this.options);n.V(t),this.I(s),this._$AH=n}}_$AC(e){let t=Me.get(e.strings);return t===void 0&&Me.set(e.strings,t=new q(e)),t}H(e){ye(this._$AH)||(this._$AH=[],this._$AR());let t=this._$AH,r,o=0;for(let n of e)o===t.length?t.push(r=new a(this.B(N()),this.B(N()),this,this.options)):r=t[o],r._$AI(n),o++;o<t.length&&(this._$AR(r&&r._$AB.nextSibling,o),t.length=o)}_$AR(e=this._$AA.nextSibling,t){for(this._$AP?.(!1,!0,t);e!==this._$AB;){let r=$e(e).nextSibling;$e(e).remove(),e=r}}setConnected(e){this._$AM===void 0&&(this.D=e,this._$AP?.(e))}},R=class{get tagName(){return this.element.tagName}get _$AU(){return this._$AM._$AU}constructor(e,t,r,o,n){this.type=1,this._$AH=i,this._$AN=void 0,this.element=e,this.name=t,this._$AM=o,this.options=n,r.length>2||r[0]!==""||r[1]!==""?(this._$AH=Array(r.length-1).fill(new String),this.strings=r):this._$AH=i}_$AI(e,t=this,r,o){let n=this.strings,s=!1;if(n===void 0)e=L(this,e,t,0),s=!U(e)||e!==this._$AH&&e!==$,s&&(this._$AH=e);else{let h=e,l,u;for(e=n[0],l=0;l<n.length-1;l++)u=L(this,h[r+l],t,l),u===$&&(u=this._$AH[l]),s||=!U(u)||u!==this._$AH[l],u===i?e=i:e!==i&&(e+=(u??"")+n[l+1]),this._$AH[l]=u}s&&!o&&this.W(e)}W(e){e===i?this.element.removeAttribute(this.name):this.element.setAttribute(this.name,e??"")}},ue=class extends R{constructor(){super(...arguments),this.type=3}W(e){this.element[this.name]=e===i?void 0:e}},be=class extends R{constructor(){super(...arguments),this.type=4}W(e){this.element.toggleAttribute(this.name,!!e&&e!==i)}},ge=class extends R{constructor(e,t,r,o,n){super(e,t,r,o,n),this.type=5}_$AI(e,t=this){if((e=L(this,e,t,0)??i)===$)return;let r=this._$AH,o=e===i&&r!==i||e.capture!==r.capture||e.once!==r.once||e.passive!==r.passive,n=e!==i&&(r===i||o);o&&this.element.removeEventListener(this.name,this,r),n&&this.element.addEventListener(this.name,this,e),this._$AH=e}handleEvent(e){typeof this._$AH=="function"?this._$AH.call(this.options?.host??this.element,e):this._$AH.handleEvent(e)}},me=class{constructor(e,t,r){this.element=e,this.type=6,this._$AN=void 0,this._$AM=t,this.options=r}get _$AU(){return this._$AM._$AU}_$AI(e){L(this,e)}};var ot=we.litHtmlPolyfillSupport;ot?.(q,B),(we.litHtmlVersions??=[]).push("3.3.3");var at=(a,e,t)=>{let r=t?.renderBefore??e,o=r._$litPart$;if(o===void 0){let n=t?.renderBefore??null;r._$litPart$=o=new B(e.insertBefore(N(),n),n,void 0,t??{})}return o._$AI(a),o},xe=globalThis;/**
 * @license
 * Copyright 2017 Google LLC
 * SPDX-License-Identifier: BSD-3-Clause
 */var c=class extends x{constructor(){super(...arguments),this.renderOptions={host:this},this.rt=void 0}createRenderRoot(){let e=super.createRenderRoot();return this.renderOptions.renderBefore??=e.firstChild,e}update(e){let t=this.render();this.hasUpdated||(this.renderOptions.isConnected=this.isConnected),super.update(e),this.rt=at(t,this.renderRoot,this.renderOptions)}connectedCallback(){super.connectedCallback(),this.rt?.setConnected(!0)}disconnectedCallback(){super.disconnectedCallback(),this.rt?.setConnected(!1)}render(){return $}};c._$litElement$=!0,c.finalized=!0,xe.litElementHydrateSupport?.({LitElement:c});var nt=xe.litElementPolyfillSupport;nt?.({LitElement:c});(xe.litElementVersions??=[]).push("4.2.2");/**
 * @license
 * Copyright 2022 Google LLC
 * SPDX-License-Identifier: BSD-3-Clause
 */var st="--popover-anchor",it=0,w=(a,e)=>{let t=`${e}-${++it}`;return a.style.setProperty(st,`--${t}`),`${t}-panel`},T=a=>a.newState==="open",lt=()=>CSS.supports("top","anchor(bottom)"),Se=8,P=(a,e,t,r=4)=>{if(lt()||!e)return;let o=e.getBoundingClientRect(),n=document.documentElement.clientWidth,s=document.documentElement.clientHeight;a.style.position="fixed",a.style.maxInlineSize=`${n-Se*2}px`;let h=a.getBoundingClientRect().height,l=s-o.bottom;h+r>l&&o.top>l?(a.style.insetBlockEnd=`${Math.round(s-o.top+r)}px`,a.style.insetBlockStart="auto"):(a.style.insetBlockStart=`${Math.round(o.bottom+r)}px`,a.style.insetBlockEnd="auto"),t==="right"?(a.style.insetInlineEnd=`${Math.max(Se,Math.round(n-o.right))}px`,a.style.insetInlineStart="auto"):(a.style.insetInlineStart=`${Math.max(Se,Math.round(o.left))}px`,a.style.insetInlineEnd="auto")},O=a=>{for(let e of["position","inset-block-start","inset-block-end","inset-inline-start","inset-inline-end","max-inline-size"])a.style.removeProperty(e)},y=p`
  [part~="panel"]:popover-open {
    margin: 0;
    min-width: var(--popover-min-width);
    padding: var(--pad-control);
    border-style: solid;
    border-color: var(--bd-surface, var(--bd, CanvasText));
    border-width: var(--bw-surface);
    border-radius: var(--roundness-control);
    background: var(--surface-3, Canvas);
    color: var(--fg, CanvasText);
    box-shadow: var(--shadow-popup);
    isolation: isolate;
    transition-property: opacity;
    transition-duration: var(--motion-duration-enter);
    transition-timing-function: var(--motion-easing-enter);
    position: fixed;
    inset: auto;
    position-anchor: var(--popover-anchor);
    top: calc(anchor(bottom) + var(--popover-offset));
    position-try-fallbacks: flip-block;
    /* L17: with no backdrop-filter a translucent step would be see-through with no frost, so the opaque card instead. */
    @supports not (backdrop-filter: blur(1px)) {
      background: var(--card, Canvas);
    }
  }
  /* L17 GLASS LAYER: the frost is a backdrop-filter on ::before, never on the host, so the host never becomes the containing block for anything fixed inside it. The layer has no background: it blurs the host's own translucent background over what lies behind. */
  [part~="panel"]:popover-open::before {
    content: "";
    position: absolute;
    inset: 0;
    z-index: -1;
    border-radius: inherit;
    pointer-events: none;
    backdrop-filter: var(--surface-blur);
  }
  @starting-style {
    [part~="panel"]:popover-open {
      opacity: 0;
    }
  }
`,I=p`
  [part~="panel"]:popover-open {
    left: anchor(left);
  }
`,Y=p`
  [part~="panel"]:popover-open {
    right: anchor(right);
  }
`,X=p`
  [part~="trigger"] {
    anchor-name: var(--popover-anchor);
  }
`;var ht=["label","combo","input","chip","chip-remove","adornment","listbox","option"],dt=["bottom-start","bottom-end","top-start","top-end"],De=a=>dt.includes(a),J=a=>`${a.kind??""}/${a.value}`,ct=a=>a.kind?`${a.label??a.value} \xB7 ${a.kind}`:a.label??a.value,pt=0,Z=class extends c{static formAssociated=!0;static styles=[m(g),p`
      :host {
        display: block;
        position: relative;
      }
      :host([disabled]) {
        pointer-events: none;
      }

      [part~="label"] {
        display: block;
        color: var(--mut, GrayText);
        font: inherit;
      }

      [part~="combo"] {
        anchor-name: var(--picker-anchor);
      }

      /* :empty, because the span is always rendered and a control with no
         adornment must not pay a gap for the one it does not have. */
      [part~="adornment"]:empty {
        display: none;
      }

      [part~="adornment"] {
        display: inline-flex;
        align-items: center;
        color: var(--mut, GrayText);
      }

      [part~="chip-remove"] {
        /* THE CONTROL FLOOR, not a token of its own. This referenced
           --picker-chip-remove-min, which theme-default never declared — so
           in ten of eleven themes it resolved to nothing and both floors were
           invalid at computed-value time. The one theme that did declare it
           aliased it to --control-min-height, which is the answer: a remove
           target is a control and takes the control floor. */
        min-height: var(--control-min-height);
        min-width: var(--control-min-height);
        padding: 0 var(--pad-control);
        border-radius: var(--roundness-pill);
        line-height: 1;
      }

      /* A popover is a centred bordered box in the UA sheet; neutralising that
         is behaviour to reset, not a look (§12.18). */
      [part~="listbox"] {
        margin: 0;
        padding: 0;
        list-style: none;
        max-height: var(--picker-max-height);
        overflow-y: auto;
        /* Longhands: the shorthand dies whole if --bw is missing, leaving the
           dropdown with no edge against the page behind it. */
        border-style: solid;
        border-color: var(--bd-surface, var(--bd, CanvasText));
        border-width: var(--bw-surface);
        border-radius: var(--roundness-control);
        background: var(--surface-3, Canvas);
        color: var(--fg, CanvasText);
        box-shadow: var(--shadow-popup);
        /* L17: DIRECT, not a layer. The listbox scrolls, and a layer inside a scroll container scrolls away
           with the options; it is in the top layer and holds only options, so being a containing block costs
           nothing here. */
        backdrop-filter: var(--surface-blur);
        position: fixed;
        inset: auto;
        position-anchor: var(--picker-anchor);
        left: anchor(left);
        min-width: anchor-size(width);
        top: calc(anchor(bottom) + var(--picker-offset));
        position-try-fallbacks: flip-block;
        /* L17: with no backdrop-filter a translucent step would be see-through with no frost, so the opaque card instead. */
        @supports not (backdrop-filter: blur(1px)) {
          background: var(--card, Canvas);
        }
      }
      :host([data-placement$="-end"]) [part~="listbox"] {
        left: auto;
        right: anchor(right);
      }

      [part~="option"] {
        display: flex;
        align-items: center;
        gap: var(--gap);
        padding: var(--pad-control);
        min-height: var(--control-min-height);
        cursor: pointer;
      }
      [part~="option"][data-active],
      [part~="option"]:hover {
        background: var(--chip, Canvas);
        color: var(--chipfg, CanvasText);
      }
    `];static properties={multiple:{type:Boolean,reflect:!0},placeholder:{type:String},label:{type:String},name:{type:String},minChars:{type:Number,attribute:"min-chars"},debounce:{type:Number},disabled:{type:Boolean,reflect:!0},placement:{type:String},offset:{type:String},items:{attribute:!1},source:{attribute:!1},selected:{attribute:!1},open:{type:Boolean,reflect:!0},_options:{state:!0},_active:{state:!0},_query:{state:!0}};#e;constructor(){super(),this.#e=this.attachInternals(),this.multiple=!1,this.placeholder="",this.label="",this.name="",this.minChars=1,this.debounce=220,this.disabled=!1,this.placement="bottom-start",this.offset="",this.items=null,this.source=null,this.selected=[],this.open=!1,this._options=[],this._active=-1,this._query="",this._timer=null,this._generation=0,this._uid=`les-picker-${++pt}`,this._borrowedName=null}connectedCallback(){super.connectedCallback(),De(this.placement)||(this.placement="bottom-start"),this.dataset.placement=this.placement,this.style.setProperty("--picker-anchor",`--${this._uid}`),this.offset&&this.style.setProperty("--picker-offset",this.offset);let e=this.getAttribute("aria-label");e&&(this._borrowedName=e,this.removeAttribute("aria-label"))}firstUpdated(){let e=this.querySelector("[data-adornment]");e&&this.renderRoot.querySelector("#adornment")?.append(e)}get isRemote(){return typeof this.source=="function"}get#t(){return this.label||this._borrowedName||this.placeholder}formResetCallback(){this.selected=[],this._query="",this.#m(),this.#r(),this.#w()}formDisabledCallback(e){this.disabled=e}willUpdate(e){e.has("selected")&&this.#r()}#r(){if(!this.name){this.#e.setFormValue(null);return}let e=new FormData;for(let t of this.selected)e.append(this.name,t.value);this.#e.setFormValue(e)}render(){let e=`${this._uid}-input`,t=`${this._uid}-listbox`;return d`
      ${this.label?d`<label part="label" for=${e}>${this.label}</label>`:i}
      <div part="combo" class="control" ?disabled=${this.disabled}>
        ${this.multiple?this.selected.map(r=>this.#o(r)):i}
        <input
          id=${e}
          part="input"
          class="control-field"
          type="text"
          role="combobox"
          autocomplete="off"
          aria-autocomplete="list"
          aria-expanded=${this.open?"true":"false"}
          aria-controls=${t}
          aria-activedescendant=${this._active>=0?`${t}-${this._active}`:i}
          aria-label=${this.label?i:this.#t||i}
          .value=${this._query}
          placeholder=${this.placeholder}
          ?disabled=${this.disabled}
          @input=${this.#s}
          @focus=${this.#l}
          @blur=${this.#d}
          @keydown=${this.#i}
        />
        <!-- After the field, where a unit or a spinner goes. -->
        <span part="adornment" id="adornment"></span>
      </div>
      <ul id=${t} part="listbox" role="listbox" popover="manual">
        ${this._options.map((r,o)=>d`
            <li
              id=${`${t}-${o}`}
              part="option"
              role="option"
              aria-selected=${o===this._active?"true":"false"}
              data-active=${o===this._active?"":i}
              @mousedown=${n=>{n.preventDefault(),this.#y(r)}}
            >
              ${ct(r)}
            </li>
          `)}
      </ul>
    `}#o(e){let t=e.label??e.value;return d`
      <span part="chip" class="pill" data-value=${e.value} data-kind=${e.kind??i}>
        ${t}
        <button
          type="button"
          part="chip-remove"
          class="control-action"
          aria-label=${`remove ${t}`}
          ?disabled=${this.disabled}
          @click=${()=>this.#k(e)}
        >
          &times;
        </button>
      </span>
    `}#a(){return this.renderRoot.querySelector('[part~="listbox"]')}#n(){return this.renderRoot.querySelector('[part~="combo"]')}#s(e){this._query=e.target.value,this._active=-1,this.#c(),e.stopPropagation(),this.dispatchEvent(new CustomEvent("input",{detail:{query:this._query},bubbles:!0,composed:!0}))}#l(){this.#c()}#d(){setTimeout(()=>this.#m(),0)}#i(e){switch(e.key){case"ArrowDown":e.preventDefault(),this.open?this.#h(1):this.#c();break;case"ArrowUp":e.preventDefault(),this.#h(-1);break;case"Enter":{let t=this._options[this._active>=0?this._active:0];t&&(e.preventDefault(),this.#y(t));break}case"Escape":this.#m();break;case"Backspace":this.multiple&&this._query===""&&this.selected.length&&this.#k(this.selected[this.selected.length-1]);break;default:break}}#h(e){let t=this._options.length;t!==0&&(this._active=this._active<0?e>0?0:t-1:(this._active+e+t)%t)}#c(){clearTimeout(this._timer);let e=this._query.trim();if(this.isRemote){if(e.length<this.minChars){this.#m();return}this._timer=setTimeout(()=>{this.#f(e)},this.debounce);return}this.#b(this.#p(e))}#p(e){let t=e.toLowerCase();return(this.items??[]).filter(r=>t===""||`${r.value}`.toLowerCase().includes(t)||`${r.label??""}`.toLowerCase().includes(t)||`${r.kind??""}`.toLowerCase().includes(t))}async#f(e){let t=++this._generation,r=[];try{r=await this.source(e)??[]}catch{r=[]}t===this._generation&&this.#b(r)}#b(e){let t=new Set(this.multiple?this.selected.map(J):[]);this._options=e.filter(r=>!t.has(J(r))).slice(0,8),this.open=this._options.length>0,this._active>=this._options.length&&(this._active=-1),this.updateComplete.then(()=>this.open?this.#v():this.#g())}#v(){let e=this.#a();e&&(e.matches(":popover-open")||e.showPopover(),this.#u(e))}#g(){let e=this.#a();e?.matches(":popover-open")&&e.hidePopover(),e&&O(e)}#u(e){let t=De(this.placement)?this.placement:"bottom-start",[r,o]=t.split("-"),n=(this.#n()??this).getBoundingClientRect(),s=e.scrollHeight,h=window.innerHeight-n.bottom,l=n.top,u=r;r==="bottom"&&s>h&&l>h&&(u="top"),r==="top"&&s>l&&h>l&&(u="bottom"),this.dataset.placement=`${u}-${o}`,P(e,this.#n()??this,o==="end"?"right":"left")}#m(){this.open=!1,this._active=-1,this.#g()}#y(e){this.selected=this.multiple?[...this.selected,e]:[e],this._query=this.multiple?"":e.label??e.value,this.#r(),this.#w(),this.multiple?this.#c():this.#m()}#k(e){this.selected=this.selected.filter(t=>J(t)!==J(e)),this.#r(),this.#w()}#w(){this.dispatchEvent(new CustomEvent("change",{detail:{selected:this.selected},bubbles:!0,composed:!0}))}};customElements.define("les-picker",Z);var ut=["services","trigger","portrait","initials","name","menu","panel","logout"],bt=a=>a.split(/\s+/).filter(Boolean).slice(0,2).map(e=>[...e][0]?.toUpperCase()??"").join(""),gt=0,Q=class extends c{static styles=[m(g),X,y,Y,p`
      :host {
        display: inline-block;
        position: relative;
      }

      [part~="trigger"] {
        display: inline-flex;
        align-items: center;
        gap: var(--gap);
        padding: 0 var(--pad-control);
      }

      [part~="portrait"],
      [part~="initials"] {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: var(--bar-portrait-size);
        height: var(--bar-portrait-size);
        border-radius: var(--roundness-pill);
        background: var(--chip, Canvas);
        color: var(--chipfg, CanvasText);
        object-fit: cover;
      }

      [part~="name"] {
        color: var(--fg, CanvasText);
      }

      [part~="services"],
      [part~="logout"] {
        display: flex;
        width: 100%;
        justify-content: flex-start;
        text-decoration: none;
      }
    `];static properties={name:{type:String},portrait:{type:String},logout:{type:String},launcher:{type:String},open:{type:Boolean,reflect:!0},_portraitFailed:{state:!0}};constructor(){super(),this.name="",this.portrait="",this.logout="",this.launcher="",this.open=!1,this._portraitFailed=!1,this._uid=`les-account-${++gt}`,this._menuId=""}connectedCallback(){super.connectedCallback(),this._menuId=w(this,"les-account")}willUpdate(e){e.has("portrait")&&(this._portraitFailed=!1)}render(){let e=this._menuId,t=this.portrait&&!this._portraitFailed;return d`
      <button
        part="trigger"
        class="control-action"
        type="button"
        popovertarget=${e}
        aria-expanded=${this.open?"true":"false"}
        aria-label=${`${this.name} \u2014 account menu`}
      >
        ${t?d`<img
              part="portrait"
              src=${this.portrait}
              alt=""
              @error=${this.#e}
            />`:d`<span part="initials" aria-hidden="true">${bt(this.name)}</span>`}
        <span part="name">${this.name}</span>
      </button>

      <div id=${e} part="menu panel" popover @toggle=${this.#t}>
        ${this.launcher?d`<a part="services" class="control-action" href=${this.launcher}
              >All services</a
            >`:i}
        ${this.logout?d`<form method="post" action=${this.logout}>
              <button part="logout" class="control-action" type="submit">Log out</button>
            </form>`:i}
      </div>
    `}#e(){this._portraitFailed=!0}#t(e){this.open=T(e);let t=e.currentTarget;this.open?P(t,this.renderRoot.querySelector('[part~="trigger"]'),"right"):O(t)}};customElements.define("les-account",Q);var mt=["burger","app","icon","pages","panel","page","current","separator","spacer","account"],ft=["trigger: account-trigger","portrait: account-portrait","initials: account-initials","name: account-name","menu: account-menu","logout: account-logout"].join(", "),vt=0,ee=class extends c{static styles=[m(g),y,I,p`
      :host {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: var(--gap) var(--gap-section);
        min-height: var(--bar-height);
        padding: 0 var(--pad-card);
        border-bottom-style: solid;
        border-bottom-color: var(--bd-surface, var(--bd, CanvasText));
        border-bottom-width: var(--bw-surface);
        background: var(--bar-bg, Canvas);
        box-shadow: var(--shadow-bar);
        color: var(--fg, CanvasText);
        font: inherit;
        position: relative;
        isolation: isolate;
        /* L17: with no backdrop-filter a translucent step would be see-through with no frost, so the opaque card instead. */
        @supports not (backdrop-filter: blur(1px)) {
          background: var(--card, Canvas);
        }
      }

      /* L17 GLASS LAYER: the frost is a backdrop-filter on ::before, never on the host, so the host never becomes the containing block for anything fixed inside it. The layer has no background: it blurs the host's own translucent background over what lies behind. */
      :host::before {
        content: "";
        position: absolute;
        inset: 0;
        z-index: -1;
        pointer-events: none;
        backdrop-filter: var(--surface-blur);
      }

      /* The mobile switch. A component writes no media query (§12.8), so the
         theme's ONE breakpoint flips these two knobs and the element only
         reads them. The keyword fallbacks are what makes the WIDE layout the
         default: with no palette linked both variables are absent and the bar
         renders app | pages | spacer | account. */
      /* The burger is the anchor, so it carries the shared anchor-name itself
         rather than through popover.ts's trigger rule — this element's
         trigger part is named burger, not trigger. */
      [part~="burger"] {
        display: var(--bar-burger-display, none);
        anchor-name: var(--popover-anchor);
      }

      /* ONE copy of the links, not two. A popover element is display:none
         until shown, and an author rule beats that UA rule — so while it is
         CLOSED this turns it back into an ordinary flex row at wide widths and
         hides it at narrow ones. When it is OPEN the rule stops matching and
         the UA puts it in the top layer. Two copies would put every link in
         the tab order twice.
         
         What this undoes is the UA stylesheet for [popover] — fixed
         positioning, an inset, a margin, padding, a border and a background,
         all of which a closed-but-shown popover would otherwise wear. It no
         longer undoes OUR panel look: popover.ts scopes that to
         :popover-open, which is where the shadow leak came from.
         
         Both halves are needed and I removed this half once: with the reset
         gone the row measured position fixed, a 3px solid UA border and an
         opaque white background — the UA rules are not the library's to
         forget. The library's own card look is scoped at its source; the
         platform's has to be undone here. */
      [part~="pages"]:not(:popover-open) {
        display: var(--bar-pages-display, flex);
        flex-wrap: wrap;
        align-items: center;
        gap: var(--gap) var(--gap-section);
        position: static;
        margin: 0;
        padding: 0;
        border: 0;
        background: none;
        inset: auto;
      }

      /* The box, the anchoring and the shadow come from popover.ts; only the
         stacking of the links is this element's business. */
      [part~="pages"]:popover-open {
        display: flex;
        flex-direction: column;
        align-items: stretch;
        gap: var(--gap);
      }

      /* R107's separator. An <hr> for its meaning, so every UA rule it wears
         is undone first: the inset border, the auto margins, the grey. Then
         ONE rule edge, drawn on the side that crosses the flow — the inline
         start in the row, the block start in the folded column. Longhands
         only: a var() inside the border shorthand kills the whole border when
         --bw is missing. With --bw at 0 the rule has no ink and the group
         boundary is the gap on either side of it, which is still a boundary. */
      [part~="separator"] {
        margin: 0;
        padding: 0;
        border-style: none;
        color: inherit;
        overflow: visible;
        flex: none;
      }
      [part~="pages"]:not(:popover-open) [part~="separator"] {
        align-self: stretch;
        inline-size: 0;
        min-block-size: 1lh;
        border-inline-start-style: solid;
        border-inline-start-color: var(--bd, CanvasText);
        border-inline-start-width: var(--bw);
      }
      [part~="pages"]:popover-open [part~="separator"] {
        align-self: stretch;
        block-size: 0;
        border-block-start-style: solid;
        border-block-start-color: var(--bd, CanvasText);
        border-block-start-width: var(--bw);
      }

      [part~="app"] {
        display: inline-flex;
        align-items: center;
        gap: var(--gap);
        font: inherit;
        font-weight: 700;
        color: var(--fg, CanvasText);
        text-decoration: none;
      }

      /* aria-hidden, because the app NAME already says which app this is —
         the emoji is recognition, not information. */
      [part~="icon"] {
        font-size: var(--bar-icon-size);
        line-height: 1;
      }

      [part~="page"] {
        color: var(--acc, LinkText);
        text-decoration: none;
        min-height: var(--control-min-height);
        display: inline-flex;
        align-items: center;
        /* TRANSPARENT on every page link, coloured on the current one. The
           indicator has to cost no height, or the current item is two pixels
           taller than its neighbours and the row of labels stops sharing a
           baseline. Derived from the hairline rather than a new token: every
           theme would have to declare a new one, and ten themes have already
           been signed off. Floored at 2px: a borderless theme sets --bw to 0,
           and twice nothing erased the only cue for the current page. */
        border-block-end-style: solid;
        border-block-end-color: transparent;
        border-block-end-width: max(2px, calc(var(--bw) * 2));
      }
      /* part="page current" is TWO part names on one element, so an exact
         [part="page"] matched neither it nor [part="current"] — the current
         link was losing every page rule. The tilde form is the whitespace-list
         match these attributes need, and is used throughout for that reason. */
      /* THREE cues, none of them sufficient alone: the ink goes to --fg while
         the others are accent-coloured, the weight goes up, and the item is
         underlined (R101 — the top-nav convention in Primer and M3). Weight
         alone is invisible at a glance and colour alone is invisible to a
         reader who cannot separate the two hues. */
      [part~="current"] {
        color: var(--fg, CanvasText);
        font-weight: 600;
        border-block-end-color: var(--acc, LinkText);
      }

      /* A real growing element, not an auto left margin. Both push the account
         right, but with an auto margin the part measures width 0 and
         flex-grow 0 — so the part named spacer was not the thing doing the
         spacing, and cron's proof caught the contract and the mechanism
         disagreeing. */
      [part~="spacer"] {
        flex: 1 1 auto;
      }

      [part~="account"] {
        display: inline-flex;
        align-items: center;
      }
    `];static properties={app:{type:String},icon:{type:String},launcher:{type:String},logout:{type:String},user:{attribute:!1},open:{type:Boolean,reflect:!0},_pages:{state:!0}};#e=null;constructor(){super(),this.app="",this.icon="",this.launcher="",this.logout="",this.user=null,this.open=!1,this._pages=[],this._uid=`les-bar-${++vt}`,this._pagesId=""}connectedCallback(){super.connectedCallback(),this._pagesId=w(this,"les-bar"),this._pages=this.#t(),this.#e=new MutationObserver(()=>{this._pages=this.#t()}),this.#e.observe(this,{childList:!0,subtree:!0,attributes:!0,attributeFilter:["aria-current","href"]})}disconnectedCallback(){super.disconnectedCallback(),this.#e?.disconnect(),this.#e=null}#t(){let e=[];for(let t of this.querySelectorAll(":scope > a, :scope > hr")){if(t.localName==="hr"){e.at(-1)?.kind==="page"&&e.push({kind:"separator"});continue}e.push({kind:"page",href:t.getAttribute("href")??"",label:(t.textContent??"").trim(),current:t.getAttribute("aria-current")==="page"})}return e.at(-1)?.kind==="separator"&&e.pop(),e}render(){let e=this._pagesId;return d`
      ${this._pages.length?d`<button
            part="burger"
            class="control-action"
            type="button"
            popovertarget=${e}
            aria-expanded=${this.open?"true":"false"}
            aria-label=${`${this.app} pages`}
          >
            ☰
          </button>`:i}

      <!-- R89(2): the app name goes HOME, to this application's root. The
           launcher moved into the account menu as its first entry, because
           "everything else on the stand" belongs with who you are rather than
           with where you are. -->
      <a part="app" href="/" aria-label=${`${this.app} \u2014 home`}
        >${this.app}${this.icon?d`<span part="icon" aria-hidden="true">${this.icon}</span>`:i}</a
      >

      <nav
        id=${e}
        part="pages panel"
        popover
        aria-label=${`${this.app} pages`}
        @toggle=${this.#r}
        @click=${this.#o}
      >
        ${this._pages.map(t=>t.kind==="separator"?d`<hr part="separator" role="separator" />`:d`<a
                part=${t.current?"page current":"page"}
                href=${t.href}
                aria-current=${t.current?"page":i}
                >${t.label}</a
              >`)}
      </nav>

      <span part="spacer"></span>

      ${this.user?d`<les-account
            part="account"
            exportparts=${ft}
            name=${this.user.name}
            portrait=${this.user.portrait??i}
            logout=${this.logout||i}
            launcher=${this.launcher||i}
          ></les-account>`:i}
    `}#r(e){this.open=T(e);let t=e.currentTarget;this.open?P(t,this.renderRoot.querySelector('[part~="burger"]'),"left"):O(t)}#o(e){if(!e.target?.closest?.("a"))return;let r=e.currentTarget;r.matches(":popover-open")&&r.hidePopover?.()}};customElements.define("les-bar",ee);var wt=["scroll","table","head","header-cell","body","row","cell","badge","label","empty"],yt=["ok","pending","running","failed","refused","skipped","disabled","unknown"],kt=new Set(["auto","span","inherit","initial","unset","revert","revert-layer","none"]),xt=a=>a.trim().toLowerCase().replace(/[^a-z0-9]+/g,"-").replace(/^-+|-+$/g,""),St=a=>/^[a-z][a-z0-9-]*$/.test(a)&&!kt.has(a),Tt=0,te=class extends c{static styles=[m(g),p`
      :host {
        display: block;
      }

      [part~="scroll"] {
        overflow: auto;
        max-width: 100%;
      }

      /* Grid, not table layout, so a row's shape is a
         grid-template-areas string the page or theme sets. The cost is that
         the implicit table roles are gone, which is why the element puts
         role= back on every part at upgrade. */
      [part~="table"] {
        display: grid;
        grid-template-columns: var(
          --table-columns,
          repeat(var(--table-col-count, 1), minmax(0, auto))
        );
        min-width: var(--table-min);
        width: 100%;
        border-collapse: collapse;
      }

      /* CARD MODE IS NOT A NARROW TABLE, and three rules written for a table
         row were wrong in it. The element already knows which it is — the
         header being hidden IS the card layout — so it says so on the host
         and these three follow.
         
         1. The scroll floor belongs to a table that scrolls sideways. A card
            stack has nothing to scroll, and the floor made a 34rem minimum
            apply at 360, which a consumer had to release at the breakpoint on
            its own side.
         2. Cells align to the BLOCK START. align-content center is right
            for a one-line row cell and wrong for a card cell holding a label
            above a value: paired cells of unequal height had their labels
            pushed down by half the difference — measured at 12px and 29.5px
            on live data, and no pairing a consumer can do would fix it,
            because the heights are the data's.
         3. No sticky column: there is no sideways scroll to stay put during,
            and a stuck cell in a card stack reads as a rendering fault. */
      :host([data-card]) [part~="table"] {
        min-width: 0;
      }

      /* A FEW COLUMNS NEED NO SCROLL FLOOR. --table-min exists so a wide
         table scrolls rather than crushing its columns, and it was applied
         whatever the table held: a two-column table at 360 was forced to
         34rem and scrolled sideways with its own header cut off — "task"
         rendering as "ta". Two columns fit a phone; the floor is for the
         tables that do not. */
      :host([data-few-columns]) [part~="table"] {
        min-width: 0;
      }

      :host([data-card]) [part~="cell"] {
        align-content: start;
      }

      :host([data-card]) .sticky-col {
        position: static;
      }

      [part~="head"],
      [part~="body"] {
        display: grid;
        grid-column: 1 / -1;
        grid-template-columns: subgrid;
      }

      [part~="row"] {
        display: grid;
        grid-column: 1 / -1;
        grid-template-columns: subgrid;
        /* The page's layout if it set one, otherwise the single row built from
           the column names at upgrade. */
        grid-template-areas: var(--table-layout, var(--table-layout-default));
        /* STRETCH, not center: a centred cell is only as tall as its content,
           so a sticky column's background stopped short of the row and
           scrolled content showed through above and below it. */
        align-items: stretch;
        /* LONGHANDS, per the rule elements.css states and build.sh now
           enforces: a border shorthand fed var(--bw) is invalid as a
           WHOLE when --bw is missing, so with no palette the table lost every
           separator. As longhands the width falls back to medium and the
           lines survive. The no-palette run caught this; the shorthand had
           looked identical in all three palette modes.

           THE SEPARATOR BELONGS TO THE ROW. Drawn per CELL it was one line per
           cell, each at that cell's own bottom edge — measured on a row whose
           cells wrap unevenly: bottoms at 1645, 1645 and 1655, so two of three
           lines sat 10px high and the hover background painted across all of
           them. A user reported it as "lines do not line up and hover
           overlaps with them". One border on the row is one line by
           construction. */
        border-block-end-style: solid;
        border-block-end-color: var(--bd, CanvasText);
        border-block-end-width: var(--bw);
      }
      [part~="body"] [part~="row"]:hover {
        background: var(--row-hover);
      }

      [part~="head"] {
        display: var(--table-header, grid);
        position: sticky;
        top: 0;
        z-index: 1;
        background: var(--table-header-bg);
      }

      [part~="header-cell"],
      [part~="cell"] {
        padding: var(--pad-control);
        min-width: 0;
        /* BREAK-WORD, not ANYWHERE. the anywhere keyword also counts every character as
           a soft wrap opportunity for INTRINSIC sizing, so a column sized
           itself to one character and long values broke mid-word even with
           room to spare: a consumer reported REGISTRY_CONNECTOR_APP_PASSWO /
           RD split across two lines in a column that could have held it.
           break-word breaks a long word only when it genuinely does not
           fit, and the scroll box above is what handles the rest. */
        overflow-wrap: break-word;
        text-align: left;
        /* A GRID rather than flex, so the content still centres vertically in
           a stretched cell WITHOUT laying the card layout's label beside its
           value — a flex cell would put them in a row and card mode needs the
           label above. */
        display: grid;
        align-content: center;
        gap: var(--gap);
      }

      [part~="header-cell"] {
        color: var(--mut, GrayText);
        font-weight: 600;
      }

      /* A sticky COLUMN: a sticky grid item in a horizontally scrolling
         container. Needs its own background or the cells it slides over show
         through, and a z-index under the header's so the corner is the
         header's. */
      /* A STICKY CELL TAKES THE SURFACE IT SITS ON. Both kinds were painted
         with the header colour, so the first column was a tonal band running
         down the whole table in every theme whose page ground differs from
         its card — the header's colour on the body's rows. A sticky cell has
         to be opaque, or the content it covers shows through while the table
         scrolls; it does not have to be the header. */
      [part~="cell"].sticky-col {
        position: sticky;
        left: 0;
        background: var(--card, Canvas);
      }

      [part~="header-cell"].sticky-col {
        position: sticky;
        left: 0;
        background: var(--table-header-bg);
      }

      /* Shown only when the header row is not, which is the card layout: a
         value with no column heading above it needs its own label.
         
         Scoped to :not([hidden]) because an AUTHOR display beats the UA's
         [hidden] { display: none } outright — cron shipped row mode with every
         cell captioned by its column name and 55px rows, while the hidden
         property was measurably set the whole time. A sheet that cannot be
         turned off by the property that exists to turn it off is the bug. */
      [part~="label"]:not([hidden]) {
        display: block;
        color: var(--mut, GrayText);
      }

      [part~="empty"] {
        padding: var(--gap-section);
        text-align: center;
        color: var(--mut, GrayText);
      }
    `];static properties={empty:{type:String},sticky:{type:String},_rows:{state:!0}};#e=null;#t=[];#r=new Set;#o=null;#a=null;#n=null;#s=null;constructor(){super(),this.empty="",this.sticky="",this._rows=0,this._uid=`les-table-${++Tt}`}render(){return d`
      <div part="scroll" id="scroll"></div>
      ${this._rows===0&&this.empty?d`<p part="empty">${this.empty}</p>`:i}
    `}firstUpdated(){let e=this.querySelector("table");if(!e){this.#n=new MutationObserver(()=>{let t=this.querySelector("table");t&&(this.#n?.disconnect(),this.#n=null,this.#l(t))}),this.#n.observe(this,{childList:!0,subtree:!0});return}this.#l(e)}#l(e){this.renderRoot.querySelector("#scroll")?.append(e),this.#e=e,this.#c(e),this.#f(e),this.#b(e),this.#v(),this.#g(),this.#o=new ResizeObserver(()=>this.#u()),this.#o.observe(this),this.#a=new MutationObserver(()=>{this.#u(),this.#g()}),this.#a.observe(this,{attributes:!0,attributeFilter:["style","class","aria-busy"]}),this.#s=new MutationObserver(()=>{this.#s?.disconnect(),this.#c(e),this.#f(e),this.#b(e),this.#v(),this.#u(),Promise.resolve().then(()=>this.#s?.observe(e,{childList:!0,subtree:!0}))}),this.#s.observe(e,{childList:!0,subtree:!0}),this.#u()}disconnectedCallback(){super.disconnectedCallback(),this.#o?.disconnect(),this.#o=null,this.#a?.disconnect(),this.#a=null,this.#n?.disconnect(),this.#n=null,this.#s?.disconnect(),this.#s=null}#d(){let e=this.#e;return e?e.querySelector("thead tr")??[...e.querySelectorAll("tr")].find(t=>t.querySelector("th"))??null:null}#i(){let e=[...this.#e?.querySelectorAll("thead th")??[]];return e.length>0?e:[...this.#d()?.querySelectorAll("th")??[]]}#h(){let e=this.#e;if(!e)return[];let t=[...e.querySelectorAll("tbody tr")];if(t.length>0)return t;let r=this.#d();return[...e.querySelectorAll("tr")].filter(o=>o!==r)}#c(e){let t=new Set;this.#t=this.#i().map((r,o)=>{let n=r.dataset.col??xt(r.textContent??""),s=`col${o+1}`,h=St(n)?n:s;return t.has(h)&&(h=s),h!==n&&this.#p(`column name "${n}" is not usable as a CSS ident or is a duplicate; using "${h}"`),t.add(h),r.dataset.col=h,h}),this.style.setProperty("--table-col-count",String(this.#t.length||1)),this.style.setProperty("--table-layout-default",`"${this.#t.join(" ")}"`)}#p(e){this.#r.has(e)||(this.#r.add(e),console.warn(`les-table: ${e}`))}#f(e){e.setAttribute("part","table"),e.setAttribute("role","table");for(let t of e.querySelectorAll("thead, tbody, tfoot"))t.setAttribute("role","rowgroup"),t.setAttribute("part",t.tagName==="THEAD"?"head":"body");for(let t of e.querySelectorAll("tr"))t.setAttribute("role","row"),t.setAttribute("part","row");this.#i().forEach(t=>{let r=t.dataset.col;t.setAttribute("role","columnheader"),t.setAttribute("part",r?`header-cell header-cell-${r}`:"header-cell"),t.style.gridArea=r??""});for(let t of this.#h())[...t.children].forEach((r,o)=>{let n=this.#t[o];r.setAttribute("role","cell"),r.setAttribute("part",n?`cell cell-${n}`:"cell"),n&&(r.dataset.col=n,r.style.gridArea=n);let s=r.dataset.status;if(s&&!r.querySelector('[part~="badge"]')){let h=yt.includes(s);h||this.#p(`data-status="${s}" is not one of the eight \xA712.11 words`);let l=document.createElement("span");l.setAttribute("part","badge"),l.classList.add("pill",h?`status-${s}`:"status-unknown"),l.append(...r.childNodes),r.append(l)}if(!r.querySelector('[part~="label"]')){let h=document.createElement("span");h.setAttribute("part","label"),h.hidden=!0,h.textContent=this.#i()[o]?.textContent?.trim()??"",r.prepend(h)}})}#b(e){if(!this.sticky)return;if(!this.#t.includes(this.sticky)){this.#p(`sticky="${this.sticky}" is not a column name`);return}for(let r of e.querySelectorAll(`[data-col="${this.sticky}"]`))r.classList.add("sticky-col");let t=this.#t.indexOf(this.sticky);for(let r of this.#h())r.children[t]?.classList.add("sticky-col")}#v(){this._rows=this.#h().length}#g(){let e=this.renderRoot.querySelector("#scroll");if(!e)return;if(this.dataset.busyInternal="",this.getAttribute("aria-busy")==="true"){e.setAttribute("aria-busy","true");let r=this.dataset.busyText;r&&(e.dataset.busyText=r)}else e.removeAttribute("aria-busy"),e.removeAttribute("data-busy-text")}#u(){let e=this.#e;if(!e)return;let t=this.#h()[0]??e.querySelector("tr");if(!t)return;let r=getComputedStyle(t).gridTemplateAreas,o=new Set(r.match(/[a-z][a-z0-9-]*/g)??[]),n=getComputedStyle(e.querySelector("thead")??e).display!=="none",s=getComputedStyle(e).gridTemplateColumns.split(/\s+/).filter(Boolean).length,l=(((r.match(/"[^"]*"/g)??[])[0]??"").match(/[a-z][a-z0-9-]*/g)??[]).length;l>0&&s>0&&s<l&&this.#p(`--table-columns has ${s} tracks but a layout row needs ${l}`);for(let u of e.querySelectorAll("td, th")){let f=u.dataset.col??"",b=o.size===0||o.has(f);u.hidden=!b;let v=u.querySelector('[part~="label"]');v&&(v.hidden=n)}this.toggleAttribute("data-card",!n),this.toggleAttribute("data-few-columns",l>0&&l<3)}};customElements.define("les-table",te);var Et=["trigger","panel","text"],re=class extends c{static styles=[m(g),X,y,I,p`
      :host {
        display: inline-block;
        position: relative;
      }

      [part~="trigger"] {
        min-height: var(--control-min-height);
        min-width: var(--control-min-height);
        padding: 0 var(--pad-control);
        border-style: none;
        background: transparent;
        color: var(--mut, GrayText);
        cursor: help;
        font: inherit;
      }

      [part~="panel"] {
        max-width: var(--field-max);
        white-space: normal;
        font-weight: 400;
      }
    `];static properties={label:{type:String},open:{type:Boolean,reflect:!0}};#e=!1;constructor(){super(),this.label="",this.open=!1,this._panelId=""}connectedCallback(){super.connectedCallback(),this._panelId=w(this,"les-hint"),this.#e=window.matchMedia("(hover: hover)").matches}render(){return d`
      <button
        part="trigger"
        class="control-action"
        type="button"
        popovertarget=${this._panelId}
        aria-expanded=${this.open?"true":"false"}
        aria-label=${this.label||"more information"}
        @pointerenter=${this.#r}
        @pointerleave=${this.#o}
        @focus=${this.#r}
        @blur=${this.#o}
      >
        ⓘ
      </button>
      <div id=${this._panelId} part="panel" popover @toggle=${this.#a}>
        <span part="text"></span>
      </div>
    `}firstUpdated(){let e=this.querySelector(":scope > [data-trigger]"),t=this.renderRoot.querySelector("button");e&&t&&(t.textContent="",t.append(e));let r=this.renderRoot.querySelector('[part~="text"]');for(let o of[...this.childNodes])o instanceof HTMLElement&&o.dataset.trigger!==void 0||r?.append(o)}#t(){return this.renderRoot.querySelector('[part~="panel"]')}#r(){if(!this.#e)return;let e=this.#t();e&&!e.matches(":popover-open")&&e.showPopover()}#o(){if(!this.#e)return;let e=this.#t();e?.matches(":popover-open")&&e.hidePopover()}#a(e){this.open=T(e)}};customElements.define("les-hint",re);var At=["text","copy","open","panel"],Ct=1200,oe=class extends c{static styles=[m(g),y,I,p`
      :host {
        display: inline-flex;
        align-items: center;
        gap: var(--gap);
        position: relative;
        anchor-name: var(--popover-anchor);
      }

      [part~="text"] {
        font: inherit;
        color: var(--acc, LinkText);
        text-align: left;
        padding: 0;
        border-style: none;
        background: transparent;
        cursor: copy;
        text-decoration: underline;
        text-decoration-style: dotted;
      }

      [part~="copy"],
      [part~="open"] {
        min-height: var(--control-min-height);
        min-width: var(--control-min-height);
        padding: 0 var(--pad-control);
        border-style: none;
        background: transparent;
        color: var(--mut, GrayText);
        cursor: pointer;
        text-decoration: none;
        font: inherit;
      }

      [part~="panel"] {
        min-width: 0;
        font-weight: 600;
      }
    `];static properties={href:{type:String},copy:{type:String},target:{type:String},label:{type:String},copied:{type:Boolean,reflect:!0},_text:{state:!0}};#e=null;#t=!1;constructor(){super(),this.href="",this.copy="",this.target="_self",this.label="",this.copied=!1,this._text="",this._panelId=""}connectedCallback(){super.connectedCallback(),this._panelId=w(this,"les-link"),this.#t=window.isSecureContext&&!!navigator.clipboard}firstUpdated(){let e=this.querySelector(":scope > a");if(!e){console.warn("les-link: no <a href> child \u2014 the anchor IS the content (README)");return}this._text=(e.textContent??"").trim(),this.href||(this.href=e.getAttribute("href")??"")}disconnectedCallback(){super.disconnectedCallback(),this.#e&&clearTimeout(this.#e)}get#r(){return this.copy||this._text}render(){let e=this.label||`copy ${this.#r}`;return d`
      <button
        part="text"
        type="button"
        aria-label=${e}
        @click=${this.#o}
      >
        ${this._text}
      </button>
      ${this.#t?d`<button part="copy" type="button" aria-label=${e} @click=${this.#a}>
            ⧉
          </button>`:i}
      ${this.href?d`<a
            part="open"
            href=${this.href}
            target=${this.target}
            rel=${this.target==="_blank"?"noopener":i}
            aria-label=${`open ${this._text}`}
            >🔗</a
          >`:i}

      <!-- popover="manual", not auto: this is a 1.2s confirmation that
           dismisses itself, and an auto popover would close whatever hint or
           menu the user already had open. -->
      <div id=${this._panelId} part="panel" popover="manual">Copied</div>
      <span class="visually-hidden" role="status" aria-live="polite"
        >${this.copied?`copied ${this.#r}`:""}</span
      >
    `}#o(e){if((e.ctrlKey||e.metaKey)&&this.href){window.open(this.href,this.target==="_blank"?"_blank":"_self");return}this.#a()}async#a(){if(this.#t){try{await navigator.clipboard.writeText(this.#r)}catch{return}this.copied=!0,this.renderRoot.querySelector('[part~="panel"]')?.showPopover(),this.#e&&clearTimeout(this.#e),this.#e=setTimeout(()=>{this.copied=!1,this.renderRoot.querySelector('[part~="panel"]')?.hidePopover()},Ct)}}};customElements.define("les-link",oe);var _t=["dialog","heading","body","actions","close"],E=[],D=()=>E.length>0,Te="les-modal-closed",ae=class extends c{static styles=[m(g),p`
      :host {
        display: contents;
      }

      [part~="dialog"] {
        /* The close control is absolutely positioned, so the dialog has to be
           its containing block or it lands relative to the viewport. */
        position: relative;
        max-width: min(var(--field-max), calc(100vw - 2 * var(--gap-section)));
        max-height: calc(100vh - 2 * var(--gap-section));
        padding: 0;
        border-style: solid;
        border-color: var(--bd-surface, var(--bd, ButtonBorder));
        border-width: var(--bw-surface);
        border-radius: var(--roundness);
        background: var(--surface-4, Canvas);
        /* L17: DIRECT, not a layer. The dialog scrolls, and a layer inside a scroll container scrolls away;
           it is in the top layer and already the containing block for its absolute close button. */
        backdrop-filter: var(--surface-blur);
        box-shadow: var(--shadow-float);
        color: var(--fg, CanvasText);
        overflow: auto;
        transition-property: opacity;
        transition-duration: var(--motion-duration-enter);
        transition-timing-function: var(--motion-easing-enter);
        /* L17: with no backdrop-filter a translucent step would be see-through with no frost, so the opaque card instead. */
        @supports not (backdrop-filter: blur(1px)) {
          background: var(--card, Canvas);
        }
      }
      @starting-style {
        [part~="dialog"][open] {
          opacity: 0;
        }
      }

      /* From the element's OWN sheet, which is the only place that reaches it:
         a document-level ::backdrop rule does not cross into a shadow tree,
         and ::backdrop cannot be named through ::part. So this is a token and
         there is no part for it. */
      [part~="dialog"]::backdrop {
        background: var(--modal-backdrop);
        /* L17: frost behind the scrim; the scrim's colour floor is unchanged. */
        backdrop-filter: var(--scrim-blur);
      }

      [part~="heading"] {
        margin: 0;
        padding: var(--gap-section);
        padding-block-end: 0;
        font-size: calc(var(--font-size) * 1.25);
      }

      [part~="body"] {
        padding: var(--gap-section);
      }

      [part~="actions"] {
        display: flex;
        flex-wrap: wrap;
        gap: var(--gap-control);
        justify-content: flex-end;
        padding: var(--gap-section);
        padding-block-start: 0;
      }

      [part~="close"] {
        position: absolute;
        inset-block-start: var(--gap-control);
        inset-inline-end: var(--gap-control);
        min-height: var(--control-min-height);
        min-width: var(--control-min-height);
        border-style: none;
        background: transparent;
        color: var(--mut, GrayText);
        font: inherit;
        cursor: pointer;
      }
    `];static properties={heading:{type:String},open:{type:Boolean,reflect:!0}};#e=!1;#t=0;constructor(){super(),this.heading="",this.open=!1}render(){return d`
      <dialog part="dialog" @close=${this.#d} @click=${this.#i}>
        ${this.heading?d`<h2 part="heading">${this.heading}</h2>`:i}
        <button part="close" type="button" aria-label="Close" @click=${()=>this.close("")}>
          <span aria-hidden="true">✕</span>
        </button>
        <div part="body" id="body"></div>
        <div part="actions" id="actions"></div>
      </dialog>
    `}firstUpdated(){this.#r()}#r(){if(this.#e)return;this.#e=!0;let e=this.querySelector(":scope > [data-actions]");e&&this.renderRoot.querySelector("#actions")?.append(e),this.renderRoot.querySelector("#body")?.append(...this.childNodes)}#o(){return this.renderRoot.querySelector("dialog")}show(){if(!this.heading){console.warn("les-modal: heading is required \u2014 an unnamed dialog announces nothing");return}this.#r();let e=this.#o();!e||e.open||(e.showModal(),this.open=!0,E.push(this),this.#a())}close(e=""){let t=this.#o();t?.open&&t.close(e)}#a(){this.#t=window.scrollY,document.documentElement.style.setProperty("overflow","hidden")}#n(){E.length>0||(document.documentElement.style.removeProperty("overflow"),window.scrollTo(0,this.#t))}#s=e=>{e.key!=="Escape"||!this.open||this.#l(this.renderRoot)&&e.preventDefault()};#l(e){for(let t of e.querySelectorAll("*"))if(t.matches(":popover-open")||t.shadowRoot&&this.#l(t.shadowRoot))return!0;return!1}#d(){let e=this.#o();this.open=!1;let t=E.indexOf(this);t>=0&&E.splice(t,1),this.#n(),E.length===0&&document.dispatchEvent(new CustomEvent(Te)),this.dispatchEvent(new CustomEvent("close",{detail:{returnValue:e?.returnValue??""},bubbles:!0,composed:!0}))}#i(e){for(let t of e.composedPath()){if(!(t instanceof HTMLElement))continue;if(t===this.#o())break;let r=t.dataset.close;if(r!==void 0){this.close(r);return}}}connectedCallback(){super.connectedCallback(),document.addEventListener("keydown",this.#s,!0)}disconnectedCallback(){super.disconnectedCallback(),document.removeEventListener("keydown",this.#s,!0);let e=E.indexOf(this);e>=0&&E.splice(e,1),this.#n()}};customElements.define("les-modal",ae);var $t=["modal","confirm","cancel","match","match-input"],ne=class extends c{static styles=[m(g),p`
      :host {
        display: contents;
      }

      /* A confirmation is a QUESTION, not a form, so it is narrower than a
         general modal — passed down as the token the dialog sizes itself on
         rather than as a width of ours. */
      [part~="modal"] {
        --field-max: 26rem;
      }

      [part~="match"] {
        display: block;
        padding: var(--gap-section);
        padding-block-start: 0;
      }

      [part~="match-input"] {
        display: block;
        width: 100%;
        margin-block-start: var(--gap);
      }
    `];static properties={message:{type:String},confirmLabel:{type:String,attribute:"confirm-label"},danger:{type:Boolean},match:{type:String},_matched:{state:!0}};#e=null;#t=!1;constructor(){super(),this.message="",this.confirmLabel="Confirm",this.danger=!1,this.match="",this._matched=!1}render(){return d`
      <!-- THE ONE RECORDED EXCEPTION TO §12.16's "no slots". The wrapped
           control must stay in the LIGHT tree, for two reasons that are spec
           rather than preference:
           
           A form owner is per TREE. A submit button moved into a shadow root
           loses the form it was declared against: its form property becomes
           null, and a form="some-id" attribute cannot resolve across the
           boundary — so "wrap the button" silently becomes "the button does
           nothing". The interception below branches on the control form, so
           the case is squarely in scope.
           
           And a framework that delegates at its root reads the RETARGETED
           event.target, which for a moved control is this host: React's
           onClick would never fire for the button it was attached to, which
           is role-ui's first adoption of this element.
           
           A slot keeps the control where the page put it: form owner intact,
           framework events intact, document.querySelector still finds it,
           and the host's capture listeners see submit because the form is a
           light-DOM descendant. Attaching a shadow root with NEITHER a slot
           nor a move is what shipped in group C: the control stopped being
           rendered at all, 0x0 with no offsetParent, and the page lost its
           delete button the moment the element upgraded. -->
      <slot></slot>

      <les-modal
        part="modal"
        exportparts="dialog,heading,body,actions,close"
        heading=${this.message||"Are you sure?"}
        @close=${this.#h}
      >
        ${this.match?d`<label part="match">
              Type <code class="mono">${this.match}</code> to confirm
              <input
                part="match-input"
                class="control-field"
                type="text"
                autocomplete="off"
                spellcheck="false"
                @input=${this.#l}
                @keydown=${this.#d}
              />
            </label>`:i}
        <div data-actions>
          <button
            part="confirm"
            class="btn ${this.danger?"danger":"primary"}"
            type="button"
            data-close="confirm"
            ?disabled=${this.match!==""&&!this._matched}
          >
            ${this.confirmLabel}
          </button>
          <button part="cancel" class="btn quiet" type="button" data-close="">Cancel</button>
        </div>
      </les-modal>
    `}connectedCallback(){super.connectedCallback(),this.addEventListener("click",this.#s,!0),this.addEventListener("submit",this.#n,!0)}disconnectedCallback(){super.disconnectedCallback(),this.removeEventListener("click",this.#s,!0),this.removeEventListener("submit",this.#n,!0)}#r(){return this.#o()?.shadowRoot?.querySelector('[part~="match-input"]')??this.renderRoot.querySelector('[part~="match-input"]')}#o(){return this.renderRoot.querySelector("les-modal")}#a(e){return e instanceof Node&&this.contains(e)}#n=e=>{if(this.#e||this.#t)return;let t=e.target;this.#a(t)&&(e.preventDefault(),e.stopPropagation(),this.#i({form:t,control:null}))};#s=e=>{if(this.#e||this.#t)return;let t=e.composedPath(),r=this.renderRoot.querySelector("les-modal");if(r&&t.includes(r))return;let o=t.find(n=>n instanceof HTMLElement&&(n.tagName==="BUTTON"||n.tagName==="A"));!o||!this.#a(o)||o instanceof HTMLButtonElement&&o.form||(e.preventDefault(),e.stopPropagation(),this.#i({form:null,control:o}))};#l(e){let t=e.target;this._matched=t.value.trim()===this.match.trim()}#d(e){e.key==="Enter"&&(e.preventDefault(),this._matched&&this.#o()?.close("confirm"))}#i(e){this.#e=e,this._matched=!1;let t=this.#r();t&&(t.value=""),this.#o()?.show(),this.match&&this.updateComplete.then(()=>this.#r()?.focus())}#h(e){let t=this.#e;this.#e=null,!(!(e.detail.returnValue==="confirm")||!t||!this.dispatchEvent(new CustomEvent("confirmed",{bubbles:!0,composed:!0,cancelable:!0})))&&(this.#t=!0,t.form?t.form.requestSubmit():t.control?.click(),setTimeout(()=>{this.#t=!1},0))}};customElements.define("les-confirm",ne);var Lt=["toast","text","dismiss"],Rt=["ok","info","err"],z=[],W=!1,ze=()=>{if(W||D())return;let a=z.shift();a&&(W=!0,a.at.raise(a.text,a.kind))};document.addEventListener(Te,()=>ze());var se=class extends c{static styles=[m(g),p`
      :host {
        display: contents;
      }

      [part~="toast"] {
        position: fixed;
        inset: auto auto var(--gap-section) var(--gap-section);
        /* A popover's UA style is inset:0 with margin:auto, which centres it.
           Both have to go or the toast lands mid-viewport over the content. */
        margin: 0;
        display: flex;
        align-items: center;
        gap: var(--gap-control);
        max-width: min(var(--toast-max), calc(100vw - 2 * var(--gap-section)));
        padding: var(--pad-control) var(--gap-section);
        border-style: solid;
        border-color: var(--bd-surface, var(--bd, ButtonBorder));
        border-width: var(--bw-surface);
        /* The status stripe is three hairlines, so a heavier theme (contrast,
           --bw: 2px) gets a heavier cue, and nothing here is a literal width. */
        border-inline-start-width: calc(var(--bw) * 3);
        border-radius: var(--roundness);
        background: var(--surface-3, Canvas);
        box-shadow: var(--shadow-float);
        color: var(--fg, CanvasText);
        font: inherit;
        transition-property: opacity;
        transition-duration: var(--motion-duration-enter);
        transition-timing-function: var(--motion-easing-enter);
        isolation: isolate;
        /* L17: with no backdrop-filter a translucent step would be see-through with no frost, so the opaque card instead. */
        @supports not (backdrop-filter: blur(1px)) {
          background: var(--card, Canvas);
        }
      }
      /* L17 GLASS LAYER: the frost is a backdrop-filter on ::before, never on the host, so the host never becomes the containing block for anything fixed inside it. The layer has no background: it blurs the host's own translucent background over what lies behind. */
      [part~="toast"]::before {
        content: "";
        position: absolute;
        inset: 0;
        z-index: -1;
        border-radius: inherit;
        pointer-events: none;
        backdrop-filter: var(--surface-blur);
      }
      @starting-style {
        [part~="toast"]:popover-open {
          opacity: 0;
        }
      }

      :host([kind="ok"]) [part~="toast"] {
        border-inline-start-color: var(--ok, LinkText);
      }

      :host([kind="err"]) [part~="toast"] {
        border-inline-start-color: var(--err, LinkText);
      }

      :host([kind="info"]) [part~="toast"] {
        border-inline-start-color: var(--acc, Highlight);
      }

      /* min-width:0 so a long message WRAPS inside the flex row instead of
         pushing the dismiss control off the end of the box. */
      [part~="text"] {
        flex: 1;
        min-width: 0;
      }

      [part~="dismiss"] {
        min-height: var(--control-min-height);
        min-width: var(--control-min-height);
        border-style: none;
        background: transparent;
        color: var(--mut, GrayText);
        font: inherit;
        cursor: pointer;
      }
    `];static properties={kind:{type:String,reflect:!0},duration:{type:Number},_text:{state:!0}};#e=null;constructor(){super(),this.kind="info",this.duration=4e3,this._text=""}render(){let e=this.kind==="err";return d`
      <div
        part="toast"
        popover="manual"
        role=${e?"alert":"status"}
        aria-live=${e?"assertive":"polite"}
      >
        <span part="text">${this._text}</span>
        ${this.duration===0?d`<button part="dismiss" type="button" aria-label="Dismiss" @click=${()=>this.hide()}>
              <span aria-hidden="true">✕</span>
            </button>`:i}
      </div>
    `}firstUpdated(){let e=this.textContent?.trim()??"";e&&(this.replaceChildren(),this.show(e,this.kind))}disconnectedCallback(){super.disconnectedCallback(),this.#e&&clearTimeout(this.#e);for(let e=z.length-1;e>=0;e--)z[e]?.at===this&&z.splice(e,1)}show(e,t){if(t&&(this.kind=Rt.includes(t)?t:"info"),D()||W){z.push({at:this,text:e,kind:this.kind});return}this.raise(e,this.kind)}raise(e,t){this.kind=t,this._text=e,W=!0,this.updateComplete.then(()=>{let r=this.renderRoot.querySelector('[part~="toast"]');r&&(r.matches(":popover-open")||r.showPopover(),this.#e&&clearTimeout(this.#e),this.duration>0&&(this.#e=setTimeout(()=>this.hide(),this.duration)))})}hide(){let e=this.renderRoot.querySelector('[part~="toast"]');e?.matches(":popover-open")&&e.hidePopover(),this._text="",this.#e&&clearTimeout(this.#e),this.#e=null,W=!1,ze()}};customElements.define("les-toast",se);var Pt=["heading","actions","panel","overflow"],ie=class extends c{static styles=[m(g),y,Y,p`
      :host {
        display: flex;
        align-items: center;
        gap: var(--gap) var(--gap-section);
        flex-wrap: wrap;
        padding-block: var(--gap);
      }

      [part~="heading"] {
        margin: 0;
        font-size: calc(var(--font-size) * 1.15);
        font-weight: 600;
      }

      /* ONE COPY of the controls, in a popover that CSS turns back into an
         ordinary row while it is closed — the bar's mechanism exactly. Two
         copies would put every control in the tab order twice, and moving
         them on a breakpoint would need a component to know the breakpoint,
         which §12.8 forbids. */
      [part~="actions"]:not(:popover-open) {
        display: var(--toolbar-actions-display, flex);
        flex-wrap: wrap;
        align-items: center;
        gap: var(--gap) var(--gap-section);
        margin: 0 0 0 auto;
        padding: 0;
        border: 0;
        background: none;
        inset: auto;
        position: static;
      }

      [part~="actions"]:popover-open {
        display: flex;
        flex-direction: column;
        align-items: stretch;
        gap: var(--gap);
      }

      [part~="overflow"] {
        display: var(--toolbar-overflow-display, none);
        margin-inline-start: auto;
      }

      /* Direct children, NOT ::slotted: §12.16 forbids slots, so the controls
         are moved into this shadow tree and are ordinary descendants here. */
      [part~="actions"] > * {
        flex: none;
      }

      /* The current tab is marked the same way in the row and in the
         overflow — a tab set that collapses into a menu where the current
         view is unmarked is worse than one that does not collapse. */
      [part~="actions"] > [aria-current] {
        font-weight: 600;
        color: var(--acc, LinkText);
        box-shadow: inset 0 -2px 0 var(--acc, LinkText);
      }
    `];static properties={heading:{type:String},open:{type:Boolean,reflect:!0}};#e=!1;constructor(){super(),this.heading="",this.open=!1,this._actionsId=""}connectedCallback(){super.connectedCallback(),this._actionsId=w(this,"les-toolbar")}render(){return d`
      ${this.heading?d`<h2 part="heading">${this.heading}</h2>`:i}
      <div
        id=${this._actionsId}
        part="actions"
        popover
        role="group"
        aria-label=${this.heading?`${this.heading} actions`:"actions"}
        @toggle=${this.#r}
      ></div>
      <button
        part="overflow"
        class="control-action"
        type="button"
        popovertarget=${this._actionsId}
        aria-expanded=${this.open?"true":"false"}
        aria-label=${this.heading?`${this.heading} actions`:"actions"}
      >
        ☰
      </button>
    `}firstUpdated(){this.#t()}#t(){if(this.#e)return;this.#e=!0,this.renderRoot.querySelector(`#${CSS.escape(this._actionsId)}`)?.append(...this.childNodes)}#r(e){this.open=T(e)}};customElements.define("les-toolbar",ie);var Ot=["panel","header","heading","actions","close","body"],le=new Set,he=class extends c{static styles=[m(g),p`
      :host {
        display: contents;
      }

      /* A MANUAL POPOVER IN THE TOP LAYER, measured against the alternatives: a
         non-modal <dialog> does not answer Escape at all, and popover=auto is
         closed by another popover opening and by an outside click — either
         would let the bar's own menu shut the panel. A manual popover never
         light-dismisses. It used to be a plain fixed box, and a fixed box is
         trapped by any ancestor that is a stacking context: inside a glass card
         (isolation: isolate) a later card painted over it — measured with
         elementFromPoint. The top layer escapes every ancestor's containing
         block and stacking context, so z-index no longer applies here.

         THE UA POPOVER SHEET, undone: [popover] is inset 0, fit-content in both
         axes, margin auto, a solid border, 0.25em of padding, overflow auto and
         Canvas colours. Each is restated so the open panel keeps exactly the box
         it had as a fixed element. */
      [part~="panel"] {
        margin: 0;
        padding: 0;
        border: 0 none;
        overflow: visible;
        height: auto;
        max-height: none;
        position: fixed;
        inset-block: var(--bar-height) 0;
        inset-inline-start: auto;
        inset-inline-end: 0;
        display: flex;
        flex-direction: column;
        width: var(--panel-width);
        max-width: 100%;
        background: var(--surface-2, Canvas);
        color: var(--fg, CanvasText);
        /* Longhands: the shorthand is invalid as a whole without --bw, which
           left the panel with no edge at all in the no-palette fallback. */
        border-inline-start-style: solid;
        border-inline-start-color: var(--bd-surface, var(--bd, ButtonBorder));
        border-inline-start-width: var(--bw-surface);
        box-shadow: var(--shadow-popup);
        transition-property: opacity;
        transition-duration: var(--motion-duration-enter);
        transition-timing-function: var(--motion-easing-enter);
        isolation: isolate;
        /* L17: with no backdrop-filter a translucent step would be see-through with no frost, so the opaque card instead. */
        @supports not (backdrop-filter: blur(1px)) {
          background: var(--card, Canvas);
        }
      }
      /* The author display above would otherwise beat the UA's display:none for
         a popover that is not open. */
      [part~="panel"]:not(:popover-open) {
        display: none;
      }
      [part~="panel"]::backdrop {
        display: none;
      }
      /* L17 GLASS LAYER: the frost is a backdrop-filter on ::before, never on the panel, and the layer has no
         background — it blurs the panel's own translucent background over what lies behind. */
      [part~="panel"]::before {
        content: "";
        position: absolute;
        inset: 0;
        z-index: -1;
        pointer-events: none;
        backdrop-filter: var(--surface-blur);
      }
      @starting-style {
        [part~="panel"] {
          opacity: 0;
        }
      }

      [part~="header"] {
        display: flex;
        align-items: center;
        gap: var(--gap);
        flex: none;
        padding: var(--pad-control) var(--pad-card);
        border-block-end-style: solid;
        border-block-end-color: var(--bd, ButtonBorder);
        border-block-end-width: var(--bw);
      }

      [part~="heading"] {
        margin: 0;
        flex: 1;
        min-width: 0;
        font-size: calc(var(--font-size) * 1.15);
        font-weight: 600;
        /* A long group name must not push the close control off the edge. */
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }

      [part~="actions"] {
        display: flex;
        align-items: center;
        gap: var(--gap);
        flex: none;
      }

      [part~="close"] {
        flex: none;
        min-height: var(--control-min-height);
        min-width: var(--control-min-height);
        border-style: none;
        background: transparent;
        color: var(--mut, GrayText);
        font: inherit;
        cursor: pointer;
      }

      /* The BODY scrolls, not the panel: the header stays put. */
      [part~="body"] {
        flex: 1;
        min-height: 0;
        overflow-y: auto;
        padding: var(--pad-card);
      }
    `];static properties={heading:{type:String},open:{type:Boolean,reflect:!0}};#e=!1;#t=!1;constructor(){super(),this.heading="",this.open=!1,this._headingId=`les-panel-${Math.random().toString(36).slice(2,8)}`}willUpdate(e){e.has("open")&&this.open&&!this.#t&&(console.warn("les-panel: open is reflected output \u2014 call show() to open it"),this.open=!1)}render(){return this.open?d`
      <aside part="panel" popover="manual" role="complementary" aria-labelledby=${this._headingId}>
        <div part="header">
          <h2 part="heading" id=${this._headingId}>${this.heading}</h2>
          <div part="actions" id="actions"></div>
          <button part="close" type="button" aria-label="Close panel" @click=${()=>this.close()}>
            <span aria-hidden="true">✕</span>
          </button>
        </div>
        <div part="body" id="body"></div>
      </aside>
    `:i}connectedCallback(){super.connectedCallback(),document.addEventListener("keydown",this.#a,!0)}disconnectedCallback(){super.disconnectedCallback(),document.removeEventListener("keydown",this.#a,!0),le.delete(this)}show(){if(!this.heading){console.warn("les-panel: heading is required \u2014 an unnamed region announces nothing");return}for(let e of le)e!==this&&e.close();this.#t=!0,this.open=!0,le.add(this),this.updateComplete.then(()=>{let e=this.renderRoot.querySelector('[part~="panel"]');e&&!e.matches(":popover-open")&&e.showPopover(),this.#r(),this.#t=!1})}close(){if(!this.open)return;this.#o();let e=this.renderRoot.querySelector('[part~="panel"]');e?.matches(":popover-open")&&e.hidePopover(),this.open=!1,le.delete(this),this.dispatchEvent(new CustomEvent("close",{bubbles:!0,composed:!0}))}#r(){if(this.#e)return;let e=this.renderRoot.querySelector("#body"),t=this.renderRoot.querySelector("#actions");if(!e||!t)return;this.#e=!0;let r=this.querySelector(":scope > [data-actions]");r&&t.append(r),e.append(...this.childNodes)}#o(){if(!this.#e)return;this.#e=!1;let e=this.renderRoot.querySelector("#actions"),t=this.renderRoot.querySelector("#body");e&&this.append(...e.childNodes),t&&this.append(...t.childNodes)}#a=e=>{e.key!=="Escape"||!this.open||D()||We(this.renderRoot,this.renderRoot.querySelector('[part~="panel"]'))||this.close()}};function We(a,e=null){for(let t of a.querySelectorAll("*"))if(t!==e&&t.matches(":popover-open")||t.shadowRoot&&We(t.shadowRoot,e))return!0;return!1}customElements.define("les-panel",he);var Lr=Object.freeze(["ok","pending","running","failed","refused","skipped","disabled","unknown"]);export{g as ELEMENTS_CSS,ut as LES_ACCOUNT_PARTS,mt as LES_BAR_PARTS,$t as LES_CONFIRM_PARTS,Et as LES_HINT_PARTS,At as LES_LINK_PARTS,_t as LES_MODAL_PARTS,Ot as LES_PANEL_PARTS,ht as LES_PICKER_PARTS,wt as LES_TABLE_PARTS,Lt as LES_TOAST_PARTS,Pt as LES_TOOLBAR_PARTS,Q as LesAccount,ee as LesBar,ne as LesConfirm,re as LesHint,oe as LesLink,ae as LesModal,he as LesPanel,Z as LesPicker,te as LesTable,se as LesToast,ie as LesToolbar,Lr as STATUS};
