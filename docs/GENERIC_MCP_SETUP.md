# Generic MCP Setup

Any MCP client supporting stdio can run:

```bash
codeintel mcp serve --stdio
```

For remote context, use `POST https://<hub>/mcp` with that project's Project Access bearer token. The same token works for every authorized client; it grants no shell access and does not return full source by default.
