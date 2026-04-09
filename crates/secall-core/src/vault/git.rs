use std::path::Path;
use std::process::Command;

pub struct VaultGit<'a> {
    vault_path: &'a Path,
}

impl<'a> VaultGit<'a> {
    pub fn new(vault_path: &'a Path) -> Self {
        Self { vault_path }
    }

    pub fn is_git_repo(&self) -> bool {
        self.vault_path.join(".git").exists()
    }

    /// vault가 rebase/merge 충돌 상태인지 확인.
    /// 충돌 상태이면 에러 메시지를 반환, 정상이면 None.
    pub fn check_conflicted_state(&self) -> Option<String> {
        if !self.is_git_repo() {
            return None;
        }

        let git_dir = self.vault_path.join(".git");

        if git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists() {
            return Some(
                "Vault repo is in a rebase state. Resolve it first:\n  \
                 cd <vault> && git rebase --abort   # or fix conflicts and: git rebase --continue"
                    .to_string(),
            );
        }

        if git_dir.join("MERGE_HEAD").exists() {
            return Some(
                "Vault repo has an unfinished merge. Resolve it first:\n  \
                 cd <vault> && git merge --abort   # or fix conflicts and: git commit"
                    .to_string(),
            );
        }

        // unmerged files 확인
        if let Ok(output) = Command::new("git")
            .args(["diff", "--name-only", "--diff-filter=U"])
            .current_dir(self.vault_path)
            .output()
        {
            let unmerged = String::from_utf8_lossy(&output.stdout);
            if !unmerged.trim().is_empty() {
                return Some(format!(
                    "Vault repo has unmerged files:\n{}\n  \
                     Resolve conflicts, then run `secall sync` again.",
                    unmerged.trim()
                ));
            }
        }

        None
    }

    /// git init + remote 설정 + .gitignore 생성
    pub fn init(&self, remote: &str) -> crate::error::Result<()> {
        if self.is_git_repo() {
            tracing::info!("vault is already a git repo");
            return Ok(());
        }

        self.run_git(&["init"])?;
        // pull()/push()가 `origin main`을 하드코딩하므로 초기화 시 브랜치를 main으로 고정.
        // `symbolic-ref`는 첫 커밋 전에도 동작하며 모든 git 버전과 호환됨.
        self.run_git(&["symbolic-ref", "HEAD", "refs/heads/main"])?;
        self.run_git(&["remote", "add", "origin", remote])?;

        // .gitignore — DB, 캐시, Obsidian 설정 제외
        let gitignore = self.vault_path.join(".gitignore");
        if !gitignore.exists() {
            std::fs::write(
                &gitignore,
                "*.db\n*.db-wal\n*.db-shm\n*.usearch\n.DS_Store\n.obsidian/\n",
            )?;
        }

        self.run_git(&["add", "."])?;
        self.run_git(&["commit", "-m", "init: seCall vault"])?;

        tracing::info!(remote, "vault git initialized");
        Ok(())
    }

    /// git pull --rebase origin main
    pub fn pull(&self) -> crate::error::Result<PullResult> {
        if !self.is_git_repo() {
            return Ok(PullResult {
                new_files: 0,
                already_up_to_date: true,
            });
        }

        let output = self.run_git(&["pull", "--rebase", "origin", "main"])?;
        let stdout = String::from_utf8_lossy(&output.stdout);

        let already_up_to_date = stdout.contains("Already up to date")
            || stdout.contains("Current branch main is up to date");

        let new_files = if !already_up_to_date {
            self.run_git(&["diff", "--stat", "HEAD@{1}", "HEAD"])
                .ok()
                .map(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .lines()
                        .filter(|l| l.contains("raw/sessions/"))
                        .count()
                })
                .unwrap_or(0)
        } else {
            0
        };

        Ok(PullResult {
            new_files,
            already_up_to_date,
        })
    }

    /// unstaged 변경이 있으면 자동 커밋. pull 전에 호출하여 rebase 충돌 방지.
    pub fn auto_commit(&self) -> crate::error::Result<bool> {
        if !self.is_git_repo() {
            return Ok(false);
        }

        let status = self.run_git(&["status", "--porcelain"])?;
        let changes = String::from_utf8_lossy(&status.stdout);
        if changes.trim().is_empty() {
            return Ok(false);
        }

        let change_count = changes.lines().count();
        tracing::info!(
            changes = change_count,
            "auto-committing unstaged vault changes before pull"
        );

        // vault 관련 파일만 stage (raw/, wiki/, index.md, log.md, .gitignore)
        self.run_git(&["add", "raw/", "wiki/", "index.md", "log.md", ".gitignore"])?;
        self.run_git(&["commit", "-m", "auto: uncommitted vault changes"])?;

        Ok(true)
    }

    /// 변경된 파일을 commit + push
    pub fn push(&self, message: &str) -> crate::error::Result<PushResult> {
        if !self.is_git_repo() {
            return Ok(PushResult { committed: 0 });
        }

        let status = self.run_git(&["status", "--porcelain"])?;
        let changes = String::from_utf8_lossy(&status.stdout);
        if changes.trim().is_empty() {
            return Ok(PushResult { committed: 0 });
        }

        let committed = changes.lines().count();

        // raw/, wiki/ 외에 vault 루트 메타데이터(index.md, log.md)도 함께 stage.
        // vault.write_session()은 index.md와 log.md를 갱신하므로 누락 시 원격 상태 불일치.
        self.run_git(&["add", "raw/", "wiki/", "index.md", "log.md"])?;
        self.run_git(&["commit", "-m", message])?;
        self.run_git(&["push", "origin", "main"])?;

        tracing::info!(committed, "vault changes pushed");
        Ok(PushResult { committed })
    }

    fn run_git(&self, args: &[&str]) -> crate::error::Result<std::process::Output> {
        let output = Command::new("git")
            .args(args)
            .current_dir(self.vault_path)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(crate::SecallError::Config(format!(
                "git {} failed: {}",
                args.join(" "),
                stderr.trim()
            )));
        }

        Ok(output)
    }
}

pub struct PullResult {
    pub new_files: usize,
    pub already_up_to_date: bool,
}

pub struct PushResult {
    pub committed: usize,
}
