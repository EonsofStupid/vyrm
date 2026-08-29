# RRFlow agent enforcement

RRFlow owns one development lifecycle. Provider integrations translate native event names and output schemas; they do not define separate workflows.

The enforced sequence is prompt-bound recall and source attunement, inspection of current code and diff, durable goal and plan, one exact attempt, pre-tool authorization, post-tool observation, decision, bounded verification, and recorded outcome. Always-on context stays short. Reusable expert procedure lives in the `rrflow-engine-development` Agent Skill. Deterministic requirements live in synchronous hooks and the RRD lifecycle supervisor.

| RRFlow event | Claude Code | Codex CLI | Gemini CLI |
| --- | --- | --- | --- |
| session start | `SessionStart` | `SessionStart` | `SessionStart` |
| prompt submitted | `UserPromptSubmit` | `UserPromptSubmit` | `BeforeAgent` |
| tool proposed | `PreToolUse` | `PreToolUse` | `BeforeTool` |
| tool observed | `PostToolUse` | `PostToolUse` | `AfterTool` |
| turn stopped | `Stop` | `Stop` | `AfterAgent` |
| pre-compaction | `PreCompact` | `PreCompact` | `PreCompress` |
| session end | `SessionEnd` | `SessionEnd` | `SessionEnd` |

Providers without a verified blocking local hook lifecycle are never reported as intercepting. They may read the shared skill and context, but project mutation must go through the RRFlow MCP dispatch or exact-argv proxy so authorization remains an engine property.

Primary references reviewed 2026-08-29:

- OpenAI Codex hooks and Agent Skills: <https://developers.openai.com/codex/hooks>, <https://developers.openai.com/codex/build-skills>
- Claude Code hooks and skills: <https://code.claude.com/docs/en/hooks>, <https://code.claude.com/docs/en/skills>
- Gemini CLI hook reference, Agent Skills, and Plan Mode: <https://github.com/google-gemini/gemini-cli/blob/main/docs/hooks/reference.md>, <https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/using-agent-skills.md>, <https://geminicli.com/docs/cli/plan-mode/>
- Agent Skills open format: <https://agentskills.io>
- MCP authorization baseline: <https://modelcontextprotocol.io/specification/2025-06-18/basic/authorization>
