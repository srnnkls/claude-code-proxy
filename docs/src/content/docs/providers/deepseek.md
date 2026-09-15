---
title: DeepSeek
description: Configure direct DeepSeek API access, model routing, streaming, tools, and provider overrides.
---

DeepSeek exposes an [Anthropic-compatible API](https://api-docs.deepseek.com/guides/anthropic_api) at `https://api.deepseek.com/anthropic`. The proxy relays Messages requests through that native endpoint rather than translating them to Chat Completions.

## Account and authentication

Create an API key in the DeepSeek platform and provide it to the proxy:

```sh
export DEEPSEEK_API_KEY=YOUR_DEEPSEEK_API_KEY
claude-code-proxy serve
```

`CCP_DEEPSEEK_API_KEY` takes precedence over `DEEPSEEK_API_KEY`. The `deepseek.apiKey` configuration key is also supported. DeepSeek does not publish an OAuth or device-login flow, so `claude-code-proxy deepseek auth status` checks configured key sources but cannot obtain a key.

## Models

The default catalog contains the current API model IDs:

- `deepseek/deepseek-flash` — DeepSeek V4.1 Flash
- `deepseek/deepseek-v4-pro` — DeepSeek V4 Pro

The `deepseek/` prefix is required. OpenCode Go already owns the bare DeepSeek IDs, and keeping those routes unchanged avoids redirecting existing configurations to a separately billed API.

Legacy IDs such as `deepseek-v4-flash` and `deepseek-v4-flash-vision-exp` remain accepted by DeepSeek and resolve to V4.1 Flash. Add any supported upstream IDs through `deepseek.models`:

```json
{
  "deepseek": {
    "models": [
      "deepseek-flash",
      "deepseek-v4-pro",
      "deepseek-v4-flash"
    ]
  }
}
```

Configured entries are upstream IDs without the `deepseek/` routing prefix.

```sh
ANTHROPIC_MODEL=deepseek/deepseek-v4-pro \
ANTHROPIC_SMALL_FAST_MODEL=deepseek/deepseek-flash \
  claude --model deepseek/deepseek-v4-pro
```

## Thinking, tools, and streaming

Thinking defaults to high effort. DeepSeek honors `output_config.effort` values `none`, `low`, `high`, and `max`; it ignores `thinking.budget_tokens`.

Text, thinking, tool calls, tool results, images, and streaming use the native Anthropic wire format. The proxy rewrites thinking carrying another proxy provider's signature to visible text before replaying the conversation, while preserving native and unsigned DeepSeek thinking. `/v1/messages/count_tokens` is handled locally.

## Compatibility limits

DeepSeek documents these Messages API differences:

- `cache_control`, `top_k`, `container`, `mcp_servers`, `service_tier`, and `disable_parallel_tool_use` are ignored.
- `document`, `search_result`, `redacted_thinking`, `code_execution_tool_result`, `mcp_tool_use`, `mcp_tool_result`, and `container_upload` content blocks are unsupported.
- `temperature` accepts 0.0–2.0. `top_p` applies only in thinking mode and has a 0.95 floor.

## Configuration

- `CCP_DEEPSEEK_API_KEY`, `DEEPSEEK_API_KEY`, or `deepseek.apiKey` supplies the key.
- `CCP_DEEPSEEK_BASE_URL` or `deepseek.baseUrl` changes the API base URL.
- `deepseek.models` replaces the default advertised model list.
