---
description: Delegate UI verification to qa-kitten (Playwright + visual analysis).
---

Hand off a UI verification task to **qa-kitten**.

## When to use

- A new component / window / dialog is implemented
- A visual regression is suspected
- An accessibility check is needed
- The cross-app injection demo needs scripted screenshots

## Usage pattern

```
invoke_agent(
  agent_name="qa-kitten",
  user_prompt="Verify <window/component> at <url-or-launch-command>. Check <specific behaviours>. Return JSON: {pass, screenshots[], findings[]}."
)
```

qa-kitten owns Playwright authorship. The implementor receives the
results and either closes the bd task or loops back to fix.
