# Agent Office

Agent Office is an Advanced page under **Agents**. It gives your agents, task runs, GPU jobs and local devices a shared visual workspace. Opening an office does not start agents or jobs.

## Create an Office

1. Switch to **Advanced** and open **Agents > Agent Office**.
2. Choose **New Office**. Pick **Small**, **Medium** or **Large** for a 16 × 12, 24 × 16 or 32 × 20 grid.
3. Enter departments, one per line. Assign agents and projects to rooms.
4. Choose **Classic**, **Pixel Art** or **Night** and finish the wizard.
5. Press **Save**. The office name can be changed above the scene.

Alternatively, expand **Office Presets**, choose **Small Studio**, **Engineering Floor**, **Agency** or **Lab**, and press **Use Preset**. This makes an editable copy. Built-in presets cannot be renamed or deleted.

## Read Live Activity

Badges show idle, thinking, running tool, waiting for approval, error or sleeping. The character list below the scene provides the same information and works with a keyboard or screen reader in every view.

Select an agent to see its latest recorded task, output and status. For the active Playground agent, **Open Run** returns to the conversation. When approval is required, **Review Approval** opens the existing prompt where you can inspect the arguments and approve or deny the tool request.

GPU jobs appear while they are queued or running. Their characters open a summary and a route to **Jobs**. Devices identify detected local hardware; their idle badge describes their office presence, not hardware utilization. Use **Performance** for resource measurements.

Connected agent adapters currently report task start and completion. Fine-grained tool and approval states are available for the mounted Playground run. External taskflow runs are not included in this version. Device and activity snapshots refresh while visible; core events refresh agent and GPU job changes immediately. The office catches up after reconnecting or returning to the window.

## Edit the Scene

Choose **Floor Plan > Advanced Editor**. You can:

- Add rooms, desks, meeting tables, kitchen furniture, plants and boards.
- Select an item on the grid or through **Selected Item**. Drag to move it on the grid, use arrow keys, or change its row, column, width and height. Rooms cannot overlap or extend beyond the office.
- Assign a project to a room. Assign a character to a desk using the field, or drag a character from the list onto a desk.
- Remove an item and use **Undo** or **Redo**. Ctrl+Z undoes; Ctrl+Shift+Z redoes when focus is outside a field.

Edits snap to whole cells. Invalid changes are rejected without changing the scene. Moving a room changes its bounds; furniture is moved separately. Undo history belongs to the current editing session. Save after editing to retain changes.

## Views and Presets

**Floor Plan** is the default. The pixel theme changes the 2D treatment. **Isometric 3D** loads the optional 3D renderer when first selected. Editing uses the floor plan; all views share the same rooms, furniture and assignments. If 3D is unavailable, return to **Floor Plan**. Rendering pauses while the window is hidden, and reduced-motion preferences disable character animation.

**Duplicate** creates a new office from the current scene. Under **Office Presets**, enter a name and use **Save as Preset** to save your own reusable layout. **Rename** updates the selected user preset. **Delete** asks before removing a saved office or user preset.

**Export JSON** downloads the current scene. **Import JSON** validates a file, gives it a fresh identity and opens an unsaved copy. Files are limited to 1 MB. Save the imported office or save it as a preset to retain it. Scene JSON stores layout and assignment identifiers, not task prompts, outputs or approval decisions.

Offices and user presets are stored as atomic JSON files in `agent-office/offices.json` and `agent-office/presets.json` inside your Harness data directory. No database migration is required.

## Control API

The following routes use the normal Control API authentication:

| Route | Purpose |
| --- | --- |
| `GET /offices` | List saved offices |
| `GET /offices/{id}` | Read one office |
| `PUT /offices/{id}` | Validate and save an office; body ID must match |
| `DELETE /offices/{id}` | Delete an office |
| `GET /office-presets` | List built-in and user presets |
| `GET /office-presets/{id}` | Read one preset |
| `PUT /office-presets/{id}` | Save a user preset |
| `DELETE /office-presets/{id}` | Delete a user preset |
| `GET /offices/activity` | Read character summaries |
| `GET /offices/runs/{id}` | Read a recorded agent task |

Reads need the `read` scope. Writes and recorded run details need `admin`. The existing `/events` stream includes additive `offices_changed` and `office_activity_changed` notifications. Scene documents use version `1`; unknown versions, invalid geometry and broken assignment references are rejected. Stable character identifiers can be read from the activity route and used in `desk_agents`; room identifiers map to project identifiers in `room_projects`.
