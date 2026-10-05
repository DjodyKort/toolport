---
description: Refresh the Moodle token through the Playwright server
allowed-tools: mcp__toolport__playwright__browser_navigate, mcp__toolport__playwright__browser_evaluate
---

# Refresh the Moodle token

- Open the Moodle login page with mcp__toolport__playwright__browser_navigate.
- Ask the user to sign in with SSO in the browser window and wait for them.
- Read the mobile-launch redirect with mcp__toolport__playwright__browser_evaluate and decode the base64 blob.
- Store the first part with toolportctl secret set moodle MOODLE_TOKEN.
