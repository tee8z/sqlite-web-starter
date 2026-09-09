-- Adopt databases created by the original demo without replacing saved data.
CREATE TABLE IF NOT EXISTS counter (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    value INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS inventory (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    quantity INTEGER NOT NULL
);

INSERT OR IGNORE INTO counter (id, value) VALUES (1, 0);

INSERT OR IGNORE INTO inventory (id, name, quantity) VALUES
    (1, 'Notebooks', 12),
    (2, 'Pencils', 48),
    (3, 'Mugs', 6);
