# Understanding Frame

Frame is a command-line utility for maintaining context and organizing hierarchical tasks, developed to solve the problem of losing track of where you are in a project after diving deep into subtasks.

## Core Concepts

### Hierarchical Context Management
- Frame stores context as a tree of nodes (called "frames")
- Each node can have a title, notes, and child nodes
- Only one frame can be active at a time
- All frames descend from an initial "root" frame

### Navigation Commands
- `push`: Create a new child frame under current frame and switch to it
- `pop`: Delete the current frame and switch to its parent
- `up`/`down`: Navigate up to parent or down to child without deletion
- `switch`: Jump to any other frame in the hierarchy
- `top`: Return to root frame

### Context Notes
- Each frame can store notes (unlimited text)
- Notes provide context for what you're working on in that frame
- Edited using your default editor or inline with the `--message` parameter

### Frame Status
- `status` command shows the current frame and its notes
- `current` command shows just the frame path (useful for shell prompts)
- `tree` command displays the entire hierarchy

## Usage Patterns

1. **Breaking Down Complex Tasks**
   - Start with a main task frame
   - Use `push` to create subtasks as needed
   - Complete subtasks and `pop` back up

2. **Maintaining Context**
   - Notes at each level explain what you're doing and why
   - When returning after time away, `status` shows where you left off
   - Breadcrumb-style path shows your position in the project hierarchy

3. **Multiple Projects**
   - Create separate branches off root for different projects
   - Use `switch` to move between projects
   - Frame remembers where you were in each project branch

## Benefits

- Prevents context loss when deep diving into complex tasks
- Creates a natural work history as you navigate the hierarchy
- Provides persistent context between work sessions
- Helps visualize complex projects as a tree structure
- Allows quick navigation between related tasks

Frame is particularly useful for software development, writing, and research projects where you might go deep into subtasks and need help remembering the bigger picture.