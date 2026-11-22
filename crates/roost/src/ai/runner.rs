use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::oneshot;

use crate::db::Database;

const MAX_LOG_LINES: usize = 200;

/// Proyecto de agente local (Antigravity, Claude Code o comando propio) gestionado 24/7.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProject {
    pub id: i64,
    pub name: String,
    pub runner: String,
    pub workdir: String,
    pub command: String,
    pub auto_restart: bool,
    pub running: bool,
    pub created_at: i64,
}

struct RunningHandle {
    stop_tx: Option<oneshot::Sender<()>>,
}

/// Gestor de procesos 24/7 para correr proyectos de Antigravity y Claude localmente desde Roost.
#[derive(Clone)]
pub struct ProjectRunner {
    pool: SqlitePool,
    running: Arc<Mutex<HashMap<i64, RunningHandle>>>,
    logs: Arc<Mutex<HashMap<i64, VecDeque<String>>>>,
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl ProjectRunner {
    pub fn new(db: &Database) -> Self {
        Self {
            pool: db.pool().clone(),
            running: Arc::new(Mutex::new(HashMap::new())),
            logs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn list_projects(&self) -> Result<Vec<AgentProject>, String> {
        let rows = sqlx::query(
            "SELECT id, name, runner, workdir, command, auto_restart, created_at
             FROM agent_projects ORDER BY id DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| "No pudimos listar los proyectos de agentes.".to_string())?;

        let running_guard = self.running.lock().expect("runner lock poisoned");
        Ok(rows
            .into_iter()
            .map(|r| {
                let id: i64 = r.get("id");
                AgentProject {
                    id,
                    name: r.get("name"),
                    runner: r.get("runner"),
                    workdir: r.get("workdir"),
                    command: r.get("command"),
                    auto_restart: r.get::<i64, _>("auto_restart") != 0,
                    running: running_guard.contains_key(&id),
                    created_at: r.get("created_at"),
                }
            })
            .collect())
    }

    pub async fn create_project(
        &self,
        name: &str,
        runner: &str,
        workdir: &str,
        command: Option<&str>,
        auto_restart: bool,
    ) -> Result<AgentProject, String> {
        let name = name.trim();
        let workdir = workdir.trim();
        if name.is_empty() || name.len() > 80 {
            return Err("El nombre del proyecto debe tener entre 1 y 80 caracteres.".to_string());
        }
        if !matches!(runner, "antigravity" | "claude" | "custom") {
            return Err("Elegí Antigravity, Claude o comando propio.".to_string());
        }
        if workdir.is_empty() || !Path::new(workdir).is_dir() {
            return Err("La carpeta del proyecto no existe en esta máquina.".to_string());
        }

        let cmd = command
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| match runner {
                "antigravity" => "agy".to_string(),
                "claude" => "claude".to_string(),
                _ => "echo 'Configurá un comando para este proyecto'".to_string(),
            });

        let created_at = now_unix();
        let res = sqlx::query(
            "INSERT INTO agent_projects (name, runner, workdir, command, auto_restart, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(name)
        .bind(runner)
        .bind(workdir)
        .bind(&cmd)
        .bind(i64::from(auto_restart))
        .bind(created_at)
        .execute(&self.pool)
        .await
        .map_err(|_| "No pudimos guardar el proyecto.".to_string())?;

        Ok(AgentProject {
            id: res.last_insert_rowid(),
            name: name.to_string(),
            runner: runner.to_string(),
            workdir: workdir.to_string(),
            command: cmd,
            auto_restart,
            running: false,
            created_at,
        })
    }

    pub async fn start_project(&self, id: i64) -> Result<(), String> {
        {
            let guard = self.running.lock().expect("runner lock poisoned");
            if guard.contains_key(&id) {
                return Ok(());
            }
        }

        let row =
            sqlx::query("SELECT workdir, command, auto_restart FROM agent_projects WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|_| "No pudimos buscar el proyecto.".to_string())?
                .ok_or_else(|| "No encontramos ese proyecto.".to_string())?;

        let workdir: String = row.get("workdir");
        let command: String = row.get("command");
        let auto_restart = row.get::<i64, _>("auto_restart") != 0;

        let (stop_tx, mut stop_rx) = oneshot::channel();
        {
            let mut guard = self.running.lock().expect("runner lock poisoned");
            guard.insert(
                id,
                RunningHandle {
                    stop_tx: Some(stop_tx),
                },
            );
        }

        let running_map = Arc::clone(&self.running);
        let logs_map = Arc::clone(&self.logs);

        tokio::spawn(async move {
            let push_log = |line: String| {
                let mut map = logs_map.lock().expect("logs lock poisoned");
                let buf = map.entry(id).or_default();
                if buf.len() >= MAX_LOG_LINES {
                    buf.pop_front();
                }
                buf.push_back(line);
            };

            loop {
                push_log(format!("[roost] Iniciando en {workdir}: {command}"));
                let child_res = Command::new("sh")
                    .arg("-c")
                    .arg(&command)
                    .current_dir(&workdir)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn();

                let mut child = match child_res {
                    Ok(c) => c,
                    Err(err) => {
                        push_log(format!("[roost] No se pudo iniciar el proceso: {err}"));
                        break;
                    }
                };

                if let Some(stdout) = child.stdout.take() {
                    let logs_clone = Arc::clone(&logs_map);
                    tokio::spawn(async move {
                        let mut lines = BufReader::new(stdout).lines();
                        while let Ok(Some(line)) = lines.next_line().await {
                            let mut map = logs_clone.lock().expect("logs lock poisoned");
                            let buf = map.entry(id).or_default();
                            if buf.len() >= MAX_LOG_LINES {
                                buf.pop_front();
                            }
                            buf.push_back(line);
                        }
                    });
                }

                let stopped = tokio::select! {
                    _ = &mut stop_rx => {
                        let _ = child.kill().await;
                        push_log("[roost] Proyecto detenido por el administrador.".to_string());
                        true
                    }
                    status = child.wait() => {
                        push_log(format!("[roost] Proceso finalizado: {status:?}"));
                        false
                    }
                };

                if stopped || !auto_restart {
                    break;
                }
                push_log(
                    "[roost] Reiniciando automáticamente en 5 segundos (modo 24/7)...".to_string(),
                );
                tokio::select! {
                    _ = &mut stop_rx => break,
                    () = tokio::time::sleep(Duration::from_secs(5)) => {}
                }
            }

            let mut guard = running_map.lock().expect("runner lock poisoned");
            guard.remove(&id);
        });

        Ok(())
    }

    pub fn stop_project(&self, id: i64) {
        let mut guard = self.running.lock().expect("runner lock poisoned");
        if let Some(mut handle) = guard.remove(&id)
            && let Some(tx) = handle.stop_tx.take()
        {
            let _ = tx.send(());
        }
    }

    pub async fn delete_project(&self, id: i64) -> Result<(), String> {
        self.stop_project(id);
        let res = sqlx::query("DELETE FROM agent_projects WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|_| "No pudimos eliminar el proyecto.".to_string())?;
        if res.rows_affected() == 0 {
            return Err("No encontramos ese proyecto.".to_string());
        }
        Ok(())
    }

    pub fn project_logs(&self, id: i64) -> Vec<String> {
        let map = self.logs.lock().expect("logs lock poisoned");
        map.get(&id)
            .map(|buf| buf.iter().cloned().collect())
            .unwrap_or_default()
    }
}
