# Proposal: Settings UI/UX improvements and new features

Date: 2026-09-30  
Status: Proposal. The first batch in §6 has been implemented except the theme preview; the rest is
not implemented.

## Basis for the assessment

The proposal is based on a screenshot the user provided and on the current Settings source, mainly
`crates/app/src/settings.rs`. The application was not run or screenshotted for this assessment.

The main direction: tighten the modal, make each setting's scope clear, and add the functions that
serve day-to-day work, while keeping onehand's minimal style.

## 1. Current problems

- The modal takes up almost the whole window, but the Appearance page holds a single choice, so the
  content looks small and scattered.
- The gap between the sidebar and the content is large, while the controls are clustered in the top
  corner.
- The theme is picked with three small buttons, which does not help anyone picture the interface
  after choosing.
- The scope a setting applies to — the whole app or a single workspace — needs to be shown more
  consistently.
- The page named `MCP Servers` currently shows connectors and their sign-in status; the name easily
  sets the expectation of a general MCP server manager.

## 2. Modal layout

| Element | Proposal |
| --- | --- |
| Size | Target about 960 × 680 px, shrinking with the window and always staying inside the visible area |
| Changing pages | Keep the modal's size stable; long content scrolls on its own |
| Sidebar | About 200–220 px wide, 16–20 px padding |
| Content | About 32 px padding, less space between the sidebar and the heading |
| Separating regions | A light border or two close background shades |
| Typography | Headings 20–22 px; labels and body text 14 px; clearer secondary text |
| Interaction states | Clearly distinguish selected, hover and keyboard focus |
| Close button | In a fixed header area, with a hit area larger than the icon |
| Sidebar footer | The app version and a way into information about the app |

The sizes above are proposed design values and need adjusting during implementation for small
windows and different interface zoom levels.

## 3. The Appearance page

Replace the three small theme buttons with three selectable tiles carrying a preview built from
native UI. Each preview mimics the sidebar, a stretch of chat and the editor. When System is chosen,
also show the actual state, for example `Currently using Dark`.

The wireframe below also illustrates the new options being proposed:

```text
Settings                         Appearance                 ×
┌───────────────────┐
│ Search settings…  │            Theme                [App]
│                   │            Applied to all windows
│ Appearance        │
│ Workspace         │            ┌────────┐ ┌────────┐ ┌────────┐
│ Agents            │            │ System │ │ Light  │ │ Dark   │
│ Connections       │            │   ✓    │ │        │ │        │
│ Shortcuts         │            └────────┘ └────────┘ └────────┘
│                   │
│                   │            Interface size       [100% ▾]
│                   │            Density          [Comfortable ▾]
│                   │
│ onehand 0.1.0     │            Changes saved
└───────────────────┘
```

There is no need for a frame around every setting. Use group headings, spacing and light dividers to
keep the page compact.

## 4. Interaction rules

| Situation | Proposed behaviour |
| --- | --- |
| Scope | An `App` or `Workspace: name` tag at the top of the page; a setting with a different scope says so on its own |
| Simple settings | Apply immediately; show `Saved` when the write succeeds, with errors next to the control |
| Agent and connection configuration forms | Use `Save / Cancel`; keep the draft when switching tabs |
| Closing the modal while editing | Ask to discard changes only when there is an unsaved draft |
| Search | One shared search box; results show the setting's name and the page holding it; picking one goes to that control |
| Keyboard navigation | Clear focus, Tab stays inside the modal, and the focused control is always visible |
| Esc key | Close the innermost interactive layer first, then Settings; handle an unsaved draft by the rule above |
| After closing | Return focus to where it was before Settings opened |

The current code already handles Esc and returns focus on close. That behaviour must be kept, and
its interaction with a form being edited must be checked.

## 5. New features by priority

| Priority | Feature | Value and scope |
| --- | --- | --- |
| P1 | Settings search | Quick access as the number of options grows |
| P1 | Interface size, with separate editor/terminal font sizes | Easier reading across different screens |
| P1 | Default agent | Fewer steps re-picking the agent when creating a session |
| P1 | Test configuration for an agent | Catch a wrong command or a missing executable before creating a session; the check runs only when the user presses it |
| P1 | Connection status | Show the status, the time of the last check, errors and `Retry` |
| P2 | Shortcut recorder | Press a key combination to assign it; reuse the existing conflict check and reset |
| P2 | Comfortable / Compact density | Suits users who prefer room and those who want to see more content |
| P2 | Agent notifications | Notify when an agent finishes or needs a reply; useful when running several sessions |
| P2 | A separate Automation page | Gather scheduling, the limit on concurrent sessions and the state of unattended runs |
| P3 | Configuration export/import | Easier moves between machines; preview changes before importing and strip secrets from the export |

### Rename MCP Servers to Connections

The current page lists connectors and their sign-in status. Renaming it to **Connections** reflects
what it actually holds.

When real MCP management is added, create a separate group with configuration, enable/disable, a
connection check and the list of tools. That is a future extension, not a capability the Settings
page has today.

## 6. Proposed scope for the first batch

1. Tighten the modal; adjust the sidebar, spacing, typography and the close button.
2. Add the theme preview and the actual theme state when System is in use.
3. Show the scope of settings and the result of saving consistently.
4. Add Settings search.
5. Add the agent configuration check and improve the connection status; rename the page to
   Connections.

Font sizes, interface density, notifications, Automation and export/import can follow in later
batches according to the priority table.

## 7. Acceptance criteria for implementation

- The modal does not overflow the window; changing pages does not make the frame jump in size.
- The sidebar and the close button stay reachable while the content page scrolls.
- The selected theme and the focus state are clearly marked.
- The user can tell the scope of a change and whether it was saved.
- A draft is not lost when switching tabs; closing the modal does not silently discard a draft.
- Search takes the user to the right setting.
- The agent/connection check shows a running state, success, or an error with a way to fix it.
- Opening Settings does not by itself trigger the newly proposed agent configuration check.

Running the application and taking screenshots for visual checks happens only with the user's
permission.
