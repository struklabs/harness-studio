Türkçe: ../tr/DESIGN.md

# Harness Studio Design System

The interface is written with **shadcn-svelte + Tailwind v4**. Components live in the project under `src/lib/components/ui/`; the theme is in `src/app.css`.

## References

| Source | What we took |
|---|---|
| Raycast | Compactness, keyboard first (⌘K), a calm dark mode, low visual noise |
| Linear | Spacing discipline, typography, alignment, quality of hover/selected states |
| OrbStack / Docker Desktop | A "control center" with a left sidebar; running servers as a live list |
| Grafana / Datadog | Metric hierarchy: one primary value is large, the others stay quiet; readable charts |
| IBM Carbon / Atlassian | Form, settings, and table layout; accessibility |

## Palette: "Graphite & Copper"

Graphite surfaces and a single accent color, copper (not neon). Light mode is a cool paper gray; **there is no pure white**.

| Token | Dark | Light | Use |
|---|---|---|---|
| `background` | `#0f1115` | `#eceef1` | Page |
| `card` | `#15181d` | `#f6f7f9` | Card/panel |
| `muted` | `#1b1f25` | `#e3e6ea` | Inactive surface, input background |
| `border` | `#252a31` | `#d3d8de` | All lines |
| `primary` | `#e27c3a` | `#b95a1c` | The **only** accent: primary action, selected item, focus |
| `success / warning / destructive / info` | green / yellow / red / blue | | Carry status meaning only |

Rules:
- Color carries meaning only. No decorative gradients, neon glow, or colored card backgrounds.
- At most **one** filled `primary` button on screen. The rest are `outline`, `secondary`, or `ghost`.
- Never hard-code colors (`#3b82f6`, `rgba(...)`); always use tokens (`bg-card`, `text-muted-foreground`, `border-border`, `bg-primary/10` …).

## Typography

- **Geist** (interface), **IBM Plex Sans** (numbers: port, size, version, token count, speed, duration, date), **Geist Mono** (code, commands, flags, paths, URLs, logs, JSON, identifiers). Fonts are bundled locally (Plex Sans is Latin + Latin Extended only, variable weight); no internet is required.
- Base size is 13 px (at the root; `rem` is 13 px). The type scale is fixed in pixels in `src/app.css` `@theme`: `text-xs` 12, `text-sm` 13, `text-base` 14, `text-lg` 16, `text-xl` 18, `text-2xl` 22, `text-3xl` 28. **No text smaller than 12 px**; arbitrary sizes such as `text-[11px]` are not used.
- Hierarchy is built with size and weight: the page title is in the title bar (not repeated on the page); the section title is `text-sm font-semibold text-foreground`; a list group label is `text-xs font-medium text-muted-foreground`; secondary information is `text-xs text-muted-foreground`.
- No UPPERCASE and no `tracking-wide` labels. Sentence case everywhere, in both TR and EN. (The English UI uses Title Case for navigation, headings, and buttons; the rule is about avoiding all-caps styling.)
- Numbers: `font-num` (IBM Plex Sans + `tnum`, aligned digits; token `--font-num`, `src/app.css`). Geist Mono's dotted zero made "8080" read like "8888" at 12 px. `font-mono` stays only for text that is copied or is code: code blocks, commands and flags (`-ngl`), paths, URLs (even with a port), logs, JSON, alias / model / quantization names, identifiers. Lines that mix numbers and text ("3 servers", "12.4 t/s") use `font-num`.

## Layout and density

- 4 px grid. Panel padding `p-4`, inside a card `p-3`, `gap-3` between form rows.
- Control height 28–32 px (`h-7`/`h-8`). Radius `rounded-md` (6 px), cards `rounded-lg`.
- Minimum window 900×600: no screen may overflow horizontally (`minmax(0,1fr)` in grids, `min-w-0`, `truncate`).
- No cards inside cards. Sections inside a panel are separated by a line (`border-b`); a box is only for a warning/error or a clickable item. No left-edge accent stripe (`border-l-4 border-l-primary`).
- A badge (`Badge`) is only for status: facts such as type, architecture, quantization, and context are written as a single line of text (`LLM · 8B · Q4_K_M · 4.9 GB`); secondary facts go under "Details".
- Description paragraphs are not piled up under a form; if needed, one line of `text-xs text-muted-foreground` or a `Tooltip`.
- "AI slop" patterns are avoided: cards with 12 px+ corners and rings (`ring`); a grid of equal KPI cards (use a single flat metrics strip instead); round speech bubbles in chat (use flat transcript rows); stacked horizontal card strips (carousels); `Sparkles`/magic-wand icons; a separate colored box behind an icon.
- Empty state = one sentence that says what to do + one action button (`ui/empty`). While a server is not running, measurement screens do not show cards full of `0.0`, `–`, or "Unavailable", or empty axes; `common/ServerOffState` is used.

## Component mapping

| Need | Use |
|---|---|
| Button | `ui/button` (`default` = primary, `outline`, `secondary`, `ghost`, `destructive`; sizes `sm`/`default`/`icon-sm`) |
| Text input | `ui/input`, `ui/textarea`, `ui/input-group`; for labels `ui/label` / `ui/field` |
| Selection | `ui/select`; for 2–5 options `ui/toggle-group` (segmented) |
| Toggle | `ui/switch` (setting), `ui/checkbox` (list selection) |
| Number range | `ui/slider` with a mono value next to it |
| Dialog | `ui/dialog` (focus trap, Esc built in); for a side panel `ui/sheet` |
| Tabs | `ui/tabs` |
| Hint | `ui/tooltip` |
| Notification | `svelte-sonner` `toast()` (the Toaster is already in the shell) |
| Table | `ui/table` |
| Status label | `ui/badge` (`outline` + status-colored text) |
| Loading | `ui/skeleton`, `ui/progress` |
| Shortcut | `ui/kbd` |

## Field help

- `common/InfoTip`: an **i** icon for short, neutral information; a tooltip on hover and keyboard focus. Takes `text` and, when needed, a `label` that identifies the field.
- `common/HelpPopover`: a **?** icon for usage steps, units, limits, and effects; click/Enter opens it, Escape closes it and returns focus. An optional `href` is a link to documentation.
- `common/FieldHelp`: contextual text through `text` for short explanations that are always needed; when a child control is given, it creates a help popover next to the control and a persistent accessible description. `id` is optional.
- The Dashboard `Field` and Settings `SettingRow` connect their existing `hint` descriptions to this shared mechanism; do not write a sufficient description a second time. `Field.Description` in `ui/field` is also connected automatically. In custom layouts, use `aria-describedby` with the id of the existing description.
- Descriptions are written in TR/EN together; units, defaults, ranges, and effects describe actual application behavior. The ids of error descriptions are preserved. Help buttons are focusable with a 28 px touch target; portal layers do not shift the layout and use the theme tokens.
- `node scripts/check-field-help.mjs --strict` scans the editable control points in scope; `--baseline <commit>` compares, and `--json` gives an inventory. A static reference check does not replace text accuracy or a live accessibility review. A single source control is counted once in loops.

## Interface level and text layers

- The single source is `AppSettings.ui_level`; an empty value means Advanced for existing users. On a new install the purpose step recommends Simple. Switching applies immediately and saves over the latest settings; it does not change other settings or the open page.
- `level: 'advanced'` is added to page, navigation-item, and group data. Menus are filtered with `visibleGroups` / `isVisible`; empty groups drop out. The command palette and deep links open every target. An Advanced page opened at Simple shows `AdvancedPageNotice`. On startup, if the last page is hidden, the first visible page is selected in its place, and the saved preference is kept.
- For a section inside a page, use `<AdvancedOnly>`; for fields, use the `advanced` prop on `SettingRow`, both `SwitchRow`s, `SettingGroup`, and the Dashboard `Field`. Hiding does not change the value; a setting that is not visible keeps being applied.
- `hint` is the description: inline at Simple, inside the (?) next to the label at Advanced. `help` is always inside the (?); when both exist, hint comes first, then help. `note` is contextual text that states a decision or result, and it always stays inline. The accessible description is kept at both levels.
- `flag` is the technical flag name: Advanced only, neutral mono text. Source badges are also shown at Advanced only; primary is not used. A field label is `text-sm font-medium text-foreground`, and descriptions and notes are at least `text-xs`.
- The decision for a new feature: "Does someone using local AI for the first time need this?" If not, mark `level` on the page or `advanced` on the section or field. In text, a description is `hint`, a long help is `help`, and information that explains a result or recommendation is `note`. The full model and the classification are in [UI_LEVELS.md](UI_LEVELS.md).

## Interaction

- Every action has visible feedback: a loading state, a toast on success, and on failure a message that says what happened and how to fix it. An error never goes only to `console.error`.
- Focus is always visible (a `focus-visible` ring). Every function can be done with the keyboard.
- Animation is short (150–200 ms) and only explains a state change; `prefers-reduced-motion` is supported.
- All text goes through `t()`; tr and en are added together (`npm run check:locales`).

## Copy language

- Use the name of what the user controls: "Start Server", "Download Model".
- A button says what it will do; the same action keeps the same name throughout a flow ("Save" → "Saved").
- An error message does not apologize and is not vague: what happened + what to do.

## Page and menu hierarchy

Connections and Settings use the same `SectionSidebar` / `ResizablePane` layout: sub-pages under section headings, a selected-page marker, keyboard focus, and an icon strip with a popover panel in narrow windows. Connections remembers the last sub-page; components that were visited stay mounted within the page. Auto-configure is the real list of tool integrations; there is no separate empty "Tool Integrations" page. MCP client status and live tool permissions are related jobs in the same component, so they share one sub-page.

The screen-by-screen layout is done for Dashboard (9), the Playground right panel (7), My Models (11), Performance (6), Analytics (5), and Settings (22 sub-pages). The last sub-page is remembered per screen; the URL hash/query, the command palette, and cross-screen links select the relevant sub-page. The Playground menu is a grouped selector above the right panel, and the chat surface is unchanged. Model Hub and Servers keep their current simple layout; the two suggestions below are not yet implemented.

| Screen / current density | Heading → sub-page hierarchy |
|---|---|
| **Done**: Dashboard (model, server form, and advanced parameters used to share one control panel) | Server → Overview, Model and Runtime, Network; Inference → Context and Memory, GPU, Sampling; Advanced → Template and Reasoning, Tools, Other Parameters. Start/stop stays in the shared bottom bar. |
| Model Hub: a single discovery list, many filters, and repository/file details | Discover → All models, Chat, Vision, Embedding and reranker, Speech; Downloads → Active, Completed. Filters stay in the list; a selected repository → Overview, Files, Hardware fit. The current screen is not a long form, so this is low priority. |
| **Done**: My Models (model types were in horizontal tabs, with definitions and local file operations together) | Models → All, Chat, Vision, Embedding, Reranker, Speech; Supporting files → Draft, Adapter, Projector; Management → Import, Model Definitions. Selected model → Overview, Files, Run definition. |
| Servers: currently a simple list; expanding server cards may get denser later | Servers → All, Running, Stopped; Selected server → Status, Configuration, Logs. Rather than adding a panel to the list right away, this is applied when the server detail grows. |
| **Done**: Playground right panel (model/preset, system prompt, sampling, tools, and other fields were in a single scroll area) | Session → Model and Preset, System Prompt; Generation → Sampling, Output Limits; Context → History and Compaction, RAG; Tools → Selection and Permissions. The chat/embedding/rerank/transcription work surfaces stay separate. |
| **Done**: Performance (Live and Benchmark tabs; charts and measurement settings were long content) | Live → Overview, Generation Speed and Latency, Memory and GPU; Benchmark → New Measurement, Results, Comparison. A running measurement job survives navigation. |
| **Done**: Analytics (Overview and Requests tabs; retention settings opened separately) | Analytics → Overview, Model and Client Breakdown; Requests → Log, Selected Request Detail; Retention → Recording and Retention Policy. The shared date filter is kept. |
| **Done**: Settings (the section side panel existed, but some sub-pages still held several long groups) | Runtime → Installed Runtime, Registry, Ollama; Storage → Directories, Hugging Face Account; Network → Listening, Access; Security → API Keys, Permissions; Gateway → Endpoint, Routing, Responses; Tools → Built-In Tools, Installed Tools; Developer → Control API, CLI; Diagnostics → Health, Reports. Appearance & Startup, Updates, Privacy, and OpenTelemetry stay single pages while they are short. The shared draft/save bar is kept. |

With every move, existing deep links, in-progress operations, TR/EN labels, and narrow-window behavior are verified together; screens are moved one at a time.
