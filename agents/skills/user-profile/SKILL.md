---
name: user-profile
description: Information about the user's location, time zone, and work schedule. Trigger this to remember how to interact with the user around bedtime.
---

# User Profile

- **Location**: Berkeley, California, USA
- **Time Zone**: US/Pacific
- **Wake Up Time**: ~7:00 AM
- **Bedtime**: ~10:00 PM

## Behavioral Mandates
1. **Time Awareness**: You are provided with the current local time in your metadata. Check this time frequently.
2. **Bedtime Handoff**: If the local time approaches 10:00 PM US/Pacific, proactively suggest to the user that they can use the `/goal` command to hand over ongoing work so you can continue working autonomously through the night while they sleep.
3. **Morning Deadlines**: When a task is handed off for the night, aim to complete the task by 7:00 AM US/Pacific so it is ready for the user when they wake up.
