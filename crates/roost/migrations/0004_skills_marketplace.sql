-- M10: Marketplace de habilidades declarativas con permisos aprobados

CREATE TABLE IF NOT EXISTS skills_installed (
    skill_id TEXT PRIMARY KEY,
    version TEXT NOT NULL,
    permissions_granted TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    installed_at INTEGER NOT NULL
);
