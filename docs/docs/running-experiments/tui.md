# RepX TUI Reference

<div align="center">
  <img src="/images/simple-tui.png" alt="RepX TUI" />
</div>

The `repx-tui` provides an interactive dashboard to monitor jobs, logs, and artifacts.

Run it with:
```bash
repx tui --lab ./result
```

## Navigation

| Key | Action |
| :--- | :--- |
| `2` | Switch to **Jobs** panel |
| `4` | Switch to **Targets** panel |
| `Space` | Open **Action Menu** (Run, Cancel, Debug, etc.) |
| `g` | Open **Go-To Menu** (Quick navigation) |
| `q` | Quit |

## Jobs Panel

When the jobs panel is focused:

| Key | Action |
| :--- | :--- |
| `j` / `↓` | Next job |
| `k` / `↑` | Previous job |
| `t` | Toggle tree view (hierarchical vs flat) |
| `.` | Toggle collapse/expand of selected tree node |
| `x` | Toggle selection and move down (multiselect) |
| `%` | Select all |
| `/` or `f` | **Filter Mode**: Type to filter jobs by name |
| `l` | Cycle forward through status filters (Pending, Running, Failed, Success) |
| `h` | Cycle backward through status filters |
| `r` | Toggle reverse sort order |

## Targets Panel

When the targets panel is focused:

| Key | Action |
| :--- | :--- |
| `j` / `↓` | Next target |
| `k` / `↑` | Previous target |
| `Enter` | Set selected target as **Active** |

## Menus

**Space Menu (Actions)**
*   `r`: **Run** selected jobs
*   `c`: **Cancel** selected jobs
*   `d`: **Debug** (inspect) selected job
*   `p`: **Path** (show output path)
*   `l`: Show global **Logs**
*   `y`: **Yank** (copy) path to clipboard
*   `e`: **Explore** output directory (opens `yazi` or shell)

**G Menu (Go To)**
*   `g`: Go to top
*   `e`: Go to end
*   `d`: Open job **Definition**
*   `l`: Open job **Logs**

## External Tools

The TUI integrates with external tools for an enhanced experience:
*   **`yazi`**: Used for file exploration when pressing `e` on a job.
*   **`$EDITOR`**: Used for opening files. Defaults to `xdg-open` locally or `vi` remotely.
