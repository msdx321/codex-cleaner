# Compact Codex memory

Treat memory as a concise recall layer for useful context learned from prior work. Required instructions belong in `AGENTS.md` or checked-in documentation, not memory.

Retain an entry only when it is:
- likely to change a future answer or action;
- supported by explicit user feedback or verified evidence;
- scoped to the exact repository, checkout, path, or environment where it applies;
- still current and not better represented by authoritative guidance;
- a verified success or a reusable, clearly labeled failure shield.

Prevent cross-project interference:
- keep global memory limited to preferences explicitly stated or repeatedly confirmed to apply across projects;
- never promote repository-specific language, framework, dependency, command, or tooling conventions to global memory without explicit cross-project evidence;
- require every project-specific entry to include `applies_to: cwd=<absolute path>` and keep different working directories in separate entries;
- keep the always-loaded `memory_summary.md` small: global preferences plus neutral routing entries labeled with their exact `cwd`;
- keep actionable project knowledge in matching `cwd` blocks in `MEMORY.md`, and never apply it when the current working directory does not match.

Prevent false outcomes:
- classify each task as `success`, `partial`, `uncertain`, or `fail` and preserve that status;
- require explicit user acceptance or relevant test, runtime, log, or tool evidence before calling an outcome successful;
- do not infer success merely because the task ended or the user moved on;
- if validation is absent, mark the result uncertain and do not promote it as durable knowledge;
- never turn a proposal, hypothesis, attempted command, or assistant explanation into a fact unless later evidence verifies it;
- retain a failed attempt only when it records the symptom, failed approach, and verified fix, pivot, or stop rule;
- let a later user correction or verified result replace the earlier claim instead of keeping both as competing truths.

Keep:
- stable user preferences and repeated corrections;
- durable project decisions, contracts, constraints, and rationale;
- current repository or system facts that are costly to rediscover;
- reusable failure patterns and verified recovery steps;
- unresolved blockers or follow-ups that still matter.

Remove:
- system or developer prompts and copied or paraphrased `AGENTS.md`, skill, plugin, MCP, or tool instructions;
- tool or skill assignment, activation, bootstrap, and routing instructions, including Superpowers;
- turn plans, progress updates, command transcripts, temporary paths, timestamps, and completed task status;
- stale or superseded facts, duplicates, speculation, and failed attempts without a reusable lesson;
- secrets, credentials, tokens, and sensitive personal data.

A tool name may remain only when it is essential to a durable finding or recovery procedure, not as an instruction to invoke the tool.

Consolidate:
- keep one canonical entry per repository or topic;
- replace older conflicting state with the latest verified state;
- keep the conclusion, scope, trigger, and minimum evidence needed to trust or reproduce it;
- keep a supporting evidence pointer only when exact details may need later verification.

Memory is advisory. The current user request, matching `AGENTS.md`, and live code, configuration, tests, and tool evidence override memory. Recheck cheap or drift-prone facts before relying on them.

This applies only to generated Codex memory. Do not change repositories, `AGENTS.md`, plugins, skills, sessions, or other user files.
