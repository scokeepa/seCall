import { ItemView, setIcon, type ViewStateResult, WorkspaceLeaf } from "obsidian";
import type SeCallPlugin from "./main";
import { SESSION_VIEW_TYPE } from "./session-view";

export const GRAPH_VIEW_TYPE = "secall-graph";

const NODE_ICONS: Record<string, string> = {
  session: "file-text",
  project: "folder",
  topic: "tag",
  tool: "wrench",
  agent: "bot",
  file: "file-code",
  issue: "alert-circle",
};

const RELATION_OPTIONS = [
  "",
  "belongs_to",
  "same_project",
  "same_day",
  "by_agent",
  "uses_tool",
  "discusses_topic",
  "modifies_file",
  "fixes_bug",
];

const MAX_VISIBLE = 50;
const MAX_BREADCRUMB = 5;

interface GraphNode {
  node_id: string;
  relation: string;
  direction: string;
  node_type?: string;
  label?: string;
}

interface GraphResult {
  query_node: string;
  depth: number;
  results: GraphNode[];
  count: number;
}

export class GraphView extends ItemView {
  plugin: SeCallPlugin;
  nodeId = "";
  depth = 1;
  relation = "";
  history: string[] = [];

  constructor(leaf: WorkspaceLeaf, plugin: SeCallPlugin) {
    super(leaf);
    this.plugin = plugin;
  }

  getViewType() {
    return GRAPH_VIEW_TYPE;
  }

  getDisplayText() {
    return "seCall Graph";
  }

  getIcon() {
    return "git-fork";
  }

  async setState(state: { nodeId?: string }, result: ViewStateResult) {
    if (state.nodeId) {
      this.nodeId = state.nodeId;
      this.history = [state.nodeId];
      await this.explore(this.nodeId);
    }
    await super.setState(state, result);
  }

  getState() {
    return { nodeId: this.nodeId };
  }

  async onOpen() {
    const container = this.containerEl.children[1] as HTMLElement;
    container.empty();

    // Search bar with node input, depth select, relation filter
    const searchBar = container.createDiv({ cls: "secall-graph-search" });
    const input = searchBar.createEl("input", {
      type: "text",
      placeholder: "Node ID (e.g. project:seCall)",
      cls: "secall-graph-input",
    });

    // Depth selector
    const depthSelect = searchBar.createEl("select", {
      cls: "secall-graph-depth",
    });
    for (const d of [1, 2, 3]) {
      const opt = depthSelect.createEl("option", {
        text: `depth ${d}`,
        value: String(d),
      });
      if (d === this.depth) opt.selected = true;
    }
    depthSelect.addEventListener("change", () => {
      this.depth = Number(depthSelect.value);
      if (this.nodeId) this.explore(this.nodeId);
    });

    // Relation filter
    const relSelect = searchBar.createEl("select", {
      cls: "secall-graph-relation",
    });
    for (const r of RELATION_OPTIONS) {
      relSelect.createEl("option", {
        text: r || "(all relations)",
        value: r,
      });
    }
    relSelect.addEventListener("change", () => {
      this.relation = relSelect.value;
      if (this.nodeId) this.explore(this.nodeId);
    });

    input.addEventListener("keydown", (e: KeyboardEvent) => {
      if (e.key === "Enter" && input.value.trim()) {
        this.nodeId = input.value.trim();
        this.history = [this.nodeId];
        this.explore(this.nodeId);
      }
    });

    container.createDiv({ cls: "secall-graph-breadcrumb" });
    container.createDiv({ cls: "secall-graph-results" });
  }

  private async explore(nodeId: string) {
    const container = this.containerEl.children[1] as HTMLElement;
    const breadcrumbEl = container.querySelector(
      ".secall-graph-breadcrumb"
    ) as HTMLElement;
    const resultsEl = container.querySelector(
      ".secall-graph-results"
    ) as HTMLElement;

    if (!breadcrumbEl || !resultsEl) return;

    // Update breadcrumb
    breadcrumbEl.empty();
    const crumbs =
      this.history.length > MAX_BREADCRUMB
        ? ["...", ...this.history.slice(-MAX_BREADCRUMB)]
        : [...this.history];

    for (let i = 0; i < crumbs.length; i++) {
      if (i > 0) breadcrumbEl.appendText(" > ");
      const crumb = crumbs[i];
      const span = breadcrumbEl.createEl("span", { text: crumb });
      if (crumb !== "..." && crumb !== nodeId) {
        span.addEventListener("click", () => {
          const idx = this.history.indexOf(crumb);
          if (idx >= 0) {
            this.history = this.history.slice(0, idx + 1);
            this.nodeId = crumb;
            this.explore(crumb);
          }
        });
      }
    }

    // Loading
    resultsEl.empty();
    resultsEl.createDiv({ text: "Loading...", cls: "secall-loading" });

    try {
      const data: GraphResult = await this.plugin.api.graph(
        nodeId,
        this.depth,
        this.relation || undefined
      );
      resultsEl.empty();

      if (data.results.length === 0) {
        resultsEl.createDiv({
          text: "No connections found.",
          cls: "secall-loading",
        });
        return;
      }

      const visible = data.results.slice(0, MAX_VISIBLE);
      this.renderNodes(resultsEl, visible);

      // Show more button if truncated
      if (data.results.length > MAX_VISIBLE) {
        const moreBtn = resultsEl.createEl("button", {
          text: `Show all (${data.results.length})`,
          cls: "secall-graph-btn",
        });
        moreBtn.addEventListener("click", () => {
          moreBtn.remove();
          this.renderNodes(resultsEl, data.results.slice(MAX_VISIBLE));
        });
      }

      // Count footer
      resultsEl.createDiv({
        text: `${data.count} connections (depth ${data.depth})`,
        cls: "secall-graph-count",
      });
    } catch (e) {
      resultsEl.empty();
      resultsEl.createEl("div", {
        text: `Error: ${e instanceof Error ? e.message : String(e)}`,
        cls: "secall-error",
      });
    }
  }

  private renderNodes(container: HTMLElement, nodes: GraphNode[]) {
    for (const node of nodes) {
      const nodeEl = container.createDiv({ cls: "secall-graph-node" });

      // Icon
      const iconName = NODE_ICONS[node.node_type || ""] || "circle";
      const iconEl = nodeEl.createSpan();
      setIcon(iconEl, iconName);

      // Node ID + label
      const idText = node.label
        ? `${node.node_id} (${node.label})`
        : node.node_id;
      nodeEl.createSpan({ text: idText, cls: "secall-graph-node-id" });

      // Relation badge
      const dir = node.direction === "out" ? "->" : "<-";
      nodeEl.createSpan({
        text: `[${dir} ${node.relation}]`,
        cls: "secall-graph-node-relation",
      });

      // Click handler
      nodeEl.addEventListener("click", () => this.handleNodeClick(node));
    }
  }

  private async handleNodeClick(node: GraphNode) {
    // Session nodes open in SessionView
    if (node.node_id.startsWith("session:")) {
      const sessionId = node.node_id.slice("session:".length);
      const leaf = this.app.workspace.getLeaf(false);
      await leaf.setViewState({
        type: SESSION_VIEW_TYPE,
        state: { sessionId },
      });
      this.app.workspace.revealLeaf(leaf);
      return;
    }

    // Other nodes: re-explore
    this.nodeId = node.node_id;
    this.history.push(node.node_id);
    await this.explore(node.node_id);
  }
}
