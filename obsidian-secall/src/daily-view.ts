import { ItemView, Notice, TFile, WorkspaceLeaf } from "obsidian";
import type SeCallPlugin from "./main";
import { SESSION_VIEW_TYPE } from "./session-view";

export const DAILY_VIEW_TYPE = "secall-daily";

interface DailySession {
  session_id: string;
  summary: string;
  turn_count: number;
  tools_used: string;
}

interface DailyData {
  date: string;
  total_sessions: number;
  filtered_sessions: number;
  topics: string[];
  projects: Record<string, DailySession[]>;
}

export class DailyView extends ItemView {
  plugin: SeCallPlugin;
  currentDate!: string;

  constructor(leaf: WorkspaceLeaf, plugin: SeCallPlugin) {
    super(leaf);
    this.plugin = plugin;
  }

  getViewType() {
    return DAILY_VIEW_TYPE;
  }

  getDisplayText() {
    return "seCall Daily";
  }

  getIcon() {
    return "calendar";
  }

  async onOpen() {
    this.currentDate = new Date().toISOString().slice(0, 10);
    await this.fetchDaily(this.currentDate);
  }

  private shiftDate(days: number) {
    const d = new Date(this.currentDate + "T00:00:00");
    d.setDate(d.getDate() + days);
    this.currentDate = d.toISOString().slice(0, 10);
    this.fetchDaily(this.currentDate);
  }

  async fetchDaily(date: string) {
    const container = this.containerEl.children[1] as HTMLElement;
    container.empty();

    // Header: nav + stats
    const header = container.createDiv({ cls: "secall-daily-header" });
    const nav = header.createDiv({ cls: "secall-daily-nav" });

    const prevBtn = nav.createEl("button", { text: "<" });
    prevBtn.addEventListener("click", () => this.shiftDate(-1));

    nav.createEl("span", { text: date, cls: "secall-daily-date" });

    const nextBtn = nav.createEl("button", { text: ">" });
    nextBtn.addEventListener("click", () => this.shiftDate(1));

    const loading = container.createDiv({
      text: "Loading...",
      cls: "secall-loading",
    });

    try {
      const data: DailyData = await this.plugin.api.daily(date);
      loading.remove();

      // Stats
      const stats = header.createDiv({ cls: "secall-daily-stats" });
      stats.setText(
        `${data.total_sessions} sessions (filtered: ${data.filtered_sessions})` +
          (data.topics.length > 0
            ? ` \u00b7 ${data.topics.slice(0, 5).join(", ")}`
            : "")
      );

      // Projects
      const projectsEl = container.createDiv({ cls: "secall-daily-projects" });
      const projectKeys = Object.keys(data.projects);

      if (projectKeys.length === 0) {
        projectsEl.createEl("div", {
          text: "No sessions for this date.",
          cls: "secall-loading",
        });
      } else {
        for (const proj of projectKeys) {
          const projEl = projectsEl.createDiv({ cls: "secall-daily-project" });
          projEl.createEl("h4", { text: proj });
          for (const s of data.projects[proj]) {
            const sessionEl = projEl.createDiv({
              cls: "secall-daily-session",
            });
            sessionEl.setText(
              `(${s.turn_count}t, ${s.tools_used}) ${s.summary || s.session_id}`
            );
            sessionEl.addEventListener("click", () =>
              this.openSession(s.session_id)
            );
          }
        }
      }

      // Actions
      const actions = container.createDiv({ cls: "secall-daily-actions" });
      const createBtn = actions.createEl("button", { text: "Create Note" });
      createBtn.addEventListener("click", () => this.createNote(data));
    } catch (e) {
      loading.remove();
      container.createEl("div", {
        text: `Error: ${e instanceof Error ? e.message : String(e)}`,
        cls: "secall-error",
      });
    }
  }

  private async openSession(sessionId: string) {
    const leaf = this.app.workspace.getLeaf(false);
    await leaf.setViewState({
      type: SESSION_VIEW_TYPE,
      state: { sessionId },
    });
    this.app.workspace.revealLeaf(leaf);
  }

  private async createNote(data: DailyData) {
    const folder = this.plugin.settings.dailyNotesFolder;
    const filePath = `${folder}/${data.date}.md`;

    // Generate markdown (same format as CLI log.rs generate_template)
    let md = `# ${data.date} \uc791\uc5c5 \uc77c\uc9c0\n\n`;
    for (const [proj, sessions] of Object.entries(data.projects)) {
      md += `## ${proj}\n`;
      for (const s of sessions) {
        md += `- (${s.turn_count}\ud134, \ub3c4\uad6c:${s.tools_used}) ${s.summary || s.session_id}\n`;
      }
      md += "\n";
    }
    if (data.topics.length > 0) {
      md += `**\uc8fc\uc694 \ud1a0\ud53d**: ${data.topics.join(", ")}\n\n`;
    }
    md += `*\ucd1d ${data.filtered_sessions}\uac1c \uc138\uc158*\n`;

    try {
      // Ensure folder exists
      if (!this.app.vault.getAbstractFileByPath(folder)) {
        await this.app.vault.createFolder(folder);
      }

      const existing = this.app.vault.getAbstractFileByPath(filePath);
      if (existing instanceof TFile) {
        await this.app.vault.modify(existing, md);
        new Notice(`Updated: ${filePath}`);
      } else {
        await this.app.vault.create(filePath, md);
        new Notice(`Created: ${filePath}`);
      }

      // Open the created file
      const file = this.app.vault.getAbstractFileByPath(filePath);
      if (file instanceof TFile) {
        await this.app.workspace.getLeaf(false).openFile(file);
      }
    } catch (e) {
      new Notice(
        `Error: ${e instanceof Error ? e.message : String(e)}`
      );
    }
  }
}
