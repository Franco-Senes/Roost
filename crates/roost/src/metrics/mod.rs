use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sysinfo::{Components, Disks, MemoryRefreshKind, RefreshKind, System};
use tokio::sync::{Notify, broadcast};

use crate::apps::RoostEvent;

/// Foto instantánea del estado del equipo (`GET /api/system` y SSE `system_metrics`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemSnapshot {
    pub cpu_percent: f32,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
    pub ram_available_mb: u64,
    pub disk_used_gb: f64,
    pub disk_total_gb: f64,
    pub temperature_c: Option<f32>,
    pub demo: bool,
}

struct Inner {
    sys: Mutex<Option<System>>,
    demo: bool,
    viewers: AtomicUsize,
    wake: Notify,
}

/// Servicio de métricas bajo demanda (`AGENTS.md` §2 y M5).
///
/// No ejecuta ningún bucle cuando `active_viewers() == 0`: la CPU de Roost
/// queda en ~0% mientras nadie tiene el panel abierto.
#[derive(Clone)]
pub struct MetricsService {
    inner: Arc<Inner>,
}

/// Guardia RAII que descuenta un espectador SSE al cerrarse la conexión.
pub struct ViewerGuard {
    inner: Arc<Inner>,
}

impl Drop for ViewerGuard {
    fn drop(&mut self) {
        self.inner.viewers.fetch_sub(1, Ordering::AcqRel);
    }
}

impl MetricsService {
    pub fn new(demo: bool) -> Self {
        let sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(sysinfo::CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything()),
        )
        .ok();
        Self {
            inner: Arc::new(Inner {
                sys: Mutex::new(sys),
                demo,
                viewers: AtomicUsize::new(0),
                wake: Notify::new(),
            }),
        }
    }

    /// Cantidad actual de navegadores mirando `/api/events`.
    pub fn active_viewers(&self) -> usize {
        self.inner.viewers.load(Ordering::Acquire)
    }

    /// Registra un nuevo cliente SSE y despierta el muestreador si estaba dormido.
    pub fn register_viewer(&self) -> ViewerGuard {
        let prev = self.inner.viewers.fetch_add(1, Ordering::AcqRel);
        if prev == 0 {
            self.inner.wake.notify_one();
        }
        ViewerGuard {
            inner: Arc::clone(&self.inner),
        }
    }

    /// Lee únicamente la memoria disponible en MB (rápido, sin refrescar CPU ni discos).
    pub fn available_ram_mb(&self) -> u64 {
        if self.inner.demo {
            return 3072;
        }
        let mut guard = self.inner.sys.lock().expect("metrics mutex poisoned");
        let Some(sys) = guard.as_mut() else {
            return 0;
        };
        sys.refresh_memory_specifics(MemoryRefreshKind::everything());
        sys.available_memory() / (1024 * 1024)
    }

    /// Toma una medición puntual de CPU, RAM, disco y temperatura.
    pub fn snapshot(&self) -> SystemSnapshot {
        if self.inner.demo {
            return SystemSnapshot {
                cpu_percent: 12.4,
                ram_used_mb: 1024,
                ram_total_mb: 4096,
                ram_available_mb: 3072,
                disk_used_gb: 48.2,
                disk_total_gb: 256.0,
                temperature_c: Some(46.5),
                demo: true,
            };
        }

        let (cpu_percent, ram_used_mb, ram_total_mb, ram_available_mb) = {
            let mut guard = self.inner.sys.lock().expect("metrics mutex poisoned");
            if let Some(sys) = guard.as_mut() {
                sys.refresh_cpu_usage();
                sys.refresh_memory_specifics(MemoryRefreshKind::everything());

                let cpu = (sys.global_cpu_usage() * 10.0).round() / 10.0;
                let total = sys.total_memory() / (1024 * 1024);
                let avail = sys.available_memory() / (1024 * 1024);
                let used = total.saturating_sub(avail);
                (cpu, used, total, avail)
            } else {
                (0.0, 0, 0, 0)
            }
        };

        let (disk_total_bytes, disk_avail_bytes): (u64, u64) = Disks::new_with_refreshed_list()
            .ok()
            .and_then(|disks| {
                disks
                    .iter()
                    .find(|d| d.mount_point() == std::path::Path::new("/"))
                    .or_else(|| disks.first())
                    .map(|d| (d.total_space(), d.available_space()))
            })
            .unwrap_or((0, 0));

        let gb = 1024.0 * 1024.0 * 1024.0;
        let disk_total_gb = ((disk_total_bytes as f64 / gb) * 10.0).round() / 10.0;
        let disk_used_gb =
            (((disk_total_bytes.saturating_sub(disk_avail_bytes)) as f64 / gb) * 10.0).round()
                / 10.0;

        let temperature_c = Components::new_with_refreshed_list()
            .ok()
            .and_then(|components| {
                components
                    .iter()
                    .filter_map(|c| c.temperature())
                    .filter(|t| *t > 0.0 && *t < 150.0)
                    .reduce(f32::max)
                    .map(|t| (t * 10.0).round() / 10.0)
            });

        SystemSnapshot {
            cpu_percent,
            ram_used_mb,
            ram_total_mb,
            ram_available_mb,
            disk_used_gb,
            disk_total_gb,
            temperature_c,
            demo: false,
        }
    }

    /// Inicia una tarea liviana que duerme por completo (`Notify`) cuando no hay
    /// espectadores y solo publica métricas cada `interval` mientras alguien mira.
    pub fn spawn_lazy_publisher(&self, events: broadcast::Sender<RoostEvent>, interval: Duration) {
        let service = self.clone();
        tokio::spawn(async move {
            loop {
                while service.active_viewers() == 0 {
                    service.inner.wake.notified().await;
                }

                while service.active_viewers() > 0 {
                    let snap = service.snapshot();
                    let _ = events.send(RoostEvent::SystemMetrics(snap));
                    tokio::time::sleep(interval).await;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_snapshot_marks_demo_flag() {
        let metrics = MetricsService::new(true);
        let snap = metrics.snapshot();
        assert!(snap.demo);
        assert!(snap.ram_total_mb > 0);
        assert!(snap.disk_total_gb > 0.0);
        assert!(snap.temperature_c.is_some());
    }

    #[test]
    fn viewer_guard_tracks_active_viewers_and_drops_to_zero() {
        let metrics = MetricsService::new(true);
        assert_eq!(metrics.active_viewers(), 0);
        {
            let _g1 = metrics.register_viewer();
            assert_eq!(metrics.active_viewers(), 1);
            {
                let _g2 = metrics.register_viewer();
                assert_eq!(metrics.active_viewers(), 2);
            }
            assert_eq!(metrics.active_viewers(), 1);
        }
        assert_eq!(metrics.active_viewers(), 0);
    }

    #[tokio::test]
    async fn lazy_publisher_emits_only_while_viewer_is_connected() {
        let metrics = MetricsService::new(true);
        let (tx, mut rx) = broadcast::channel(16);
        metrics.spawn_lazy_publisher(tx, Duration::from_millis(20));

        // Sin espectadores no debe emitir nada.
        assert!(
            tokio::time::timeout(Duration::from_millis(60), rx.recv())
                .await
                .is_err()
        );

        // Con espectador emite métricas en vivo.
        let guard = metrics.register_viewer();
        let ev = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("debe emitir con un espectador activo")
            .unwrap();
        assert!(matches!(ev, RoostEvent::SystemMetrics(_)));

        drop(guard);
        assert_eq!(metrics.active_viewers(), 0);
    }
}
