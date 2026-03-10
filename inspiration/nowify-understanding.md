# Understanding Nowify

Check it out on GitHub.

Time-management is hard. So I delegated the hard parts to a computer program.

nowify runs my life.

Every morning, I up start nowify and it guides me through my day: "what am I supposed to be doing now?"

nowify counts every second. If I forget about nowify, it yells at me.

## General Logic

Here's the general logic:

- Prompt the user with the highest priority routine from the queue. Routines usually occur daily or hourly.
- If "skip" is selected, prompt this routine again in 40 minutes.
- If "done" is selected on a repeated routine, prompt again in 60 minutes.
- If "done" is selected on a non-repeated routine, prompt again tomorrow.
- If the routine takes more than N minutes, start beeping.
- If "not-done" is selected, start the beep timer again in N minutes.

Because humans change more often than computer programs, there are meta-routines to update the routines:

- "What parts of life have been neglected? Adjust routine priorities."
- "What routines are frequently being skipped? Ask spouse for advice."

Very simple. Very effective.

## Core Principles

1. **"Don't make me think"**
   - Design your life structure once, then let the system calculate what to do next
   - Routines are pre-defined in a configuration file

2. **Push over pull**
   - Active and even "annoying" systems work better than passive ones
   - The system prompts you to take action rather than waiting for you to check it

3. **Active feedback**
   - Provides simple statistics to help you see patterns in your productivity
   - Visual heatmaps show when and how you spend your time

4. **Meta-routines**
   - Includes prompts to review and update your routines themselves
   - Acknowledges that systems need maintenance and evolution

5. **Bite-sized modularity**
   - Tasks broken into small, manageable chunks (typically 25 minutes)
   - Simpler is better than fancy

## Key Components

### Routines Configuration
- Defined in a CSV file at `~/.config/nowify/routines.csv`
- Each routine includes:
  - Days of week when it applies
  - Time window when it's relevant
  - Duration (typically in minutes)
  - Importance score (1-4)
  - Unique identifier
  - Description/prompt

### Task Flow
1. System suggests the next routine based on:
   - Current time of day
   - Which routines are scheduled for today
   - Which routines haven't been completed yet
   - Priority of remaining routines

2. For each routine, you respond with:
   - `done`: Completed the task
   - `skip`: Skipping this time
   - `wait`: Will do it soon, remind me

### Statistics Display
- Shows a visual heatmap of completed activities
- Organizes by time of day (vertical) and day (horizontal)
- Uses different symbols to indicate completion intensity (based on score)
- Helps identify patterns in productivity and routine adherence

### "Annoy" Feature
- Actively reminds you of overdue routines with sound alerts
- Makes the system push-based rather than requiring you to check it

## Usage Benefits

- Reduces decision fatigue by telling you what to focus on next
- Creates structured routine without requiring constant planning
- Provides accountability through visual feedback
- Balances structure with flexibility (you can skip or defer tasks)
- Helps establish and maintain productive habits
- Makes time use visible, improving awareness of how you spend your day

The system is particularly well-suited for people who:
- Want structured routines but struggle with consistency
- Need external accountability
- Benefit from breaking work into small sessions
- Want to track patterns in their productivity over time