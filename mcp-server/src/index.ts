import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";
import { execSync } from "child_process";

function gw(args: string, cwd?: string): string {
  try {
    return execSync(`gw ${args}`, {
      encoding: "utf-8",
      timeout: 30000,
      cwd: cwd || process.cwd(),
    }).trim();
  } catch (e: any) {
    const stderr = e.stderr?.trim();
    const stdout = e.stdout?.trim();
    throw new Error(stderr || stdout || e.message);
  }
}

const server = new McpServer({
  name: "gw",
  version: "0.1.0",
});

server.tool("gw_tree", "Show all stacks with branches and commits", {}, async () => {
  return { content: [{ type: "text", text: gw("tree") }] };
});

server.tool(
  "gw_tree_pr",
  "Show all stacks with branches, commits, and PR status from GitHub",
  {},
  async () => {
    return { content: [{ type: "text", text: gw("tree --pr") }] };
  }
);

server.tool(
  "gw_stack_create",
  "Create a new stack off the base branch",
  { name: z.string().describe("Name for the stack (also creates a branch)") },
  async ({ name }) => {
    return { content: [{ type: "text", text: gw(`stack create ${name}`) }] };
  }
);

server.tool(
  "gw_branch_create",
  "Add a new branch to the current stack (must be on the leaf branch)",
  { name: z.string().describe("Branch name") },
  async ({ name }) => {
    return { content: [{ type: "text", text: gw(`branch create ${name}`) }] };
  }
);

server.tool(
  "gw_switch",
  "Switch to a branch tracked by gw",
  { branch: z.string().describe("Branch name to switch to") },
  async ({ branch }) => {
    return { content: [{ type: "text", text: gw(`switch ${branch}`) }] };
  }
);

server.tool(
  "gw_rebase",
  "Propagate rebases to all descendant branches in the current stack",
  {},
  async () => {
    return { content: [{ type: "text", text: gw("rebase") }] };
  }
);

server.tool(
  "gw_sync",
  "Fetch base branch, detect squash merges, and rebase remaining stack",
  {},
  async () => {
    return { content: [{ type: "text", text: gw("sync") }] };
  }
);

server.tool(
  "gw_push",
  "Push the current branch (force-with-lease if diverged)",
  {},
  async () => {
    return { content: [{ type: "text", text: gw("push --yes") }] };
  }
);

server.tool("gw_stack_list", "List all stacks", {}, async () => {
  return { content: [{ type: "text", text: gw("stack list") }] };
});

async function main() {
  const transport = new StdioServerTransport();
  await server.connect(transport);
}

main().catch(console.error);
