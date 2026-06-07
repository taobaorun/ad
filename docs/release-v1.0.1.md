# AD v1.0.1 — Release Report

> A macOS desktop app for managing Claude Code settings — profiles, per-project config, skill management, and terminal integration, all from your menubar.

**Release date**: 2026-06-07  
**Platform**: macOS (Apple Silicon)  
**Tech stack**: Tauri 2.x · React 18 · TypeScript 5 · Rust · CodeMirror 6  

---

## What is AD?

AD is a menubar-resident desktop app for power users of [Claude Code](https://docs.anthropic.com/en/docs/claude-code). It replaces the tedious cycle of hand-editing `~/.claude/settings.json` with a structured UI: layered templates, per-project configuration, skill management, and one-click terminal launching.

**Target audience**: Developers who use Claude Code daily and need to switch between multiple accounts, API endpoints, model preferences, or environment variables across different projects.

---

## Feature Overview

### 1. Project Management

| Feature | Description |
|---------|-------------|
| **Auto-discovery** | Scans `~/.claude/projects/` and registered roots to find projects automatically |
| **Per-project config** | Each project owns its `.claude/settings.json` + `settings.local.json` — AD edits them in place |
| **Inline editor** | Three-tab editor (Shared / Local / Env) with CodeMirror 6 JSON editing and KV environment table |
| **Template system** | Profiles serve as read-only templates — apply a template to initialize or switch a project's config |
| **Conflict resolution** | When switching templates, inline conflict resolver handles merge conflicts per-field |
| **Project sidebar** | Compact project list with status ring, search, and `⌘1-9` keyboard shortcuts |

### 2. Skill Management

| Feature | Description |
|---------|-------------|
| **Skill Sources** | Add git repositories or local directories as skill sources |
| **Git clone** | Clones repos to `~/.ad/skill-library/` with loading indicator |
| **Auto-name** | Extracts repository name from git URL as default source name |
| **Global skills** | Toggle skills globally — symlinked to `~/.claude/skills/`, visible to all projects |
| **Project skills** | Per-project allowlist/blocklist mode for fine-grained control |
| **Batch toggle** | Enable/disable all skills in a source with one click |
| **Auto-update** | Optionally pull latest on AD launch |

### 3. Terminal Integration

| Feature | Description |
|---------|-------------|
| **4 backends** | Ghostty, cmux, Terminal.app, and Custom command template |
| **One-click launch** | Open any project in your preferred terminal with `claude` pre-configured |
| **Custom templates** | `{{cwd}}` and `{{cmd}}` placeholders for full control |
| **Settings UI** | Configure terminal preferences, Claude binary path, and custom command |

### 4. System Tray

| Feature | Description |
|---------|-------------|
| **Menubar icon** | Brand icon with active-profile color ring indicator |
| **Quick switch** | Switch profiles directly from the tray menu |
| **Hide to tray** | Closing the window hides to tray instead of quitting |
| **Live sync** | Tray state stays in sync with the main UI |

### 5. Settings Window

| Feature | Description |
|---------|-------------|
| **General** | Language switching (Chinese / English), dark mode |
| **Terminal** | Backend selection, Claude binary path, custom command template |
| **Skills** | Skill source management and global skill toggles |
| **Shortcuts** | Customizable global hotkey (default `⌥⌘A`) to summon/hide AD |
| **Legacy** | Backward-compatible global activation for simple use cases |

### 6. UX & Platform

| Feature | Description |
|---------|-------------|
| **Dark mode** | Full dark/light theme support |
| **i18n** | Chinese and English with real-time cross-window sync |
| **Command Palette** | `⌘K` for quick actions: switch project, switch template, edit template, add project, view history |
| **Global hotkey** | System-wide shortcut to show/hide AD from any app |
| **Atomic writes** | APFS rename semantics — crash-safe file operations |
| **Auto-backup** | Every settings overwrite is backed up first |
| **History** | Full operation history with restore capability |
| **Splash screen** | Skeleton UI eliminates white flash on startup |
| **Rounded icon** | macOS-native squircle icon matching dock aesthetics |

---

## Architecture

```
┌──────────────────────────────────────────────────┐
│                   macOS                          │
│  ┌──────────┐  ┌──────────┐  ┌──────────────┐   │
│  │  Main    │  │ Settings │  │ System Tray  │   │
│  │  Window  │  │  Window  │  │              │   │
│  │ (App.tsx)│  │(Settings │  │  (tray/)     │   │
│  │          │  │ App.tsx) │  │              │   │
│  └────┬─────┘  └────┬─────┘  └──────┬───────┘   │
│       │              │               │           │
│       └──────────────┼───────────────┘           │
│                      │                           │
│            localStorage sync                     │
│       (dark mode, lang, terminal, hotkey)         │
│                      │                           │
│  ┌───────────────────┴───────────────────────┐   │
│  │          Tauri IPC (invoke)                │   │
│  └───────────────────┬───────────────────────┘   │
│                      │                           │
│  ┌───────────────────┴───────────────────────┐   │
│  │              Rust Backend                  │   │
│  │  commands/ ─ skills, profiles, projects,   │   │
│  │              apply, settings, discover,    │   │
│  │              terminal, history, shortcut   │   │
│  │  fs/       ─ atomic write, git, merge      │   │
│  │  tray/     ─ menubar icon + menu           │   │
│  └───────────────────────────────────────────┘   │
│                      │                           │
│  ┌───────────────────┴───────────────────────┐   │
│  │            File System                     │   │
│  │  ~/.ad/          ─ AD data (profiles,      │   │
│  │                    backups, history,        │   │
│  │                    skill-library, state)    │   │
│  │  ~/.claude/      ─ CC data (read-only,     │   │
│  │                    except skills/ symlinks  │   │
│  │                    and legacy activation)   │   │
│  │  <project>/      ─ .claude/settings*.json  │   │
│  │    .claude/        (source of truth)        │   │
│  └───────────────────────────────────────────┘   │
└──────────────────────────────────────────────────┘
```

---

## Version History

| Version | Milestone | Key Changes |
|---------|-----------|-------------|
| **v0.1.0** | Initial release | Profile CRUD, JSON editor, one-click activation, auto-backup, operation history |
| **v0.1.x** | System tray | Menubar icon with color ring, tray menu quick-switch, hide-to-tray |
| **v0.2** | Layered profiles | Three-layer profile structure (Shared/Local/Env), data migration to `~/.ad/`, project rename to AD |
| **v0.3** | UI redesign | Zinc/Indigo theme, dark mode, two-column layout, Chinese/English i18n |
| **v0.4** | Per-project config | Project as source of truth, template system, inline editor, conflict resolver, auto-discovery, Command Palette |
| **v0.5** | Terminal integration | 4 terminal backends (Ghostty/cmux/Terminal.app/Custom), one-click launch |
| **v0.6** | UX polish | Auto-jump on add, global hotkey, search prominence, splash screen |
| **v0.7** | Editor upgrade | Monaco to CodeMirror 6, WKWebView fix, lazy loading |
| **v1.0.0** | Skill management | Skill source CRUD, git clone, global/project skill toggles, batch operations |
| **v1.0.1** | Polish | Auto-name from URL, clone loading indicator, full i18n for Skills, cross-window language sync, rounded icon |

---

## Data Layout

```
~/.ad/
├── profiles/           # <id>.json — layered templates
├── backups/            # auto-backups before every overwrite
├── history/            # <ts>.<uuid>.json — operation log
├── skill-library/      # <source-id>/ — cloned git repos
├── state/
│   ├── projects.json       # project-template mapping
│   ├── scan_roots.json     # auto-discovery roots
│   └── skill-sources.json  # registered skill sources
└── active              # pointer to current legacy profile

~/.claude/
├── settings.json       # read by CC (AD writes via legacy path only)
├── settings.local.json # per-machine overrides
├── skills/             # symlinks managed by AD
└── projects/           # scanned for auto-discovery
```

---

## Build & Install

```bash
# Prerequisites: Rust toolchain + pnpm 9 + Xcode CLI tools
pnpm install
pnpm tauri build

# Install (unsigned local build)
cp -R src-tauri/target/release/bundle/macos/AD.app /Applications/
xattr -dr com.apple.quarantine /Applications/AD.app
```

---

## What's Next

- Skill browsing and discovery UI
- Plugin management (MCP servers)
- Profile import/export for team sharing
- Enhanced project status indicators
