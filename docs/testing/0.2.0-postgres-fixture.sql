-- Run once in each newly created disposable acceptance database using psql.
-- No existing schema or data is dropped by this fixture.
CREATE SCHEMA release_check;
CREATE SCHEMA release_other;

CREATE TABLE release_check.parents (
    tenant_id integer NOT NULL,
    id integer NOT NULL,
    label varchar(120) NOT NULL,
    PRIMARY KEY (tenant_id, id)
);
INSERT INTO release_check.parents VALUES
    (1, 1, 'Parent one'), (1, 2, 'Parent two'), (2, 1, 'Other tenant');

CREATE TABLE release_check.items (
    id integer PRIMARY KEY,
    tenant_id integer NOT NULL DEFAULT 1,
    parent_id integer NOT NULL DEFAULT 1,
    previous_id integer REFERENCES release_check.items(id),
    name varchar(120) NOT NULL UNIQUE,
    note text DEFAULT 'default note',
    amount numeric(18,2) NOT NULL DEFAULT 0,
    enabled boolean NOT NULL DEFAULT true,
    payload json,
    markup xml,
    raw bytea,
    occurred_at timestamptz,
    CONSTRAINT items_parent_fk FOREIGN KEY (tenant_id, parent_id)
        REFERENCES release_check.parents(tenant_id, id),
    CONSTRAINT items_amount_check CHECK (amount >= 0)
);
COMMENT ON TABLE release_check.items IS 'FastDB disposable acceptance fixture';
COMMENT ON COLUMN release_check.items.note IS E'Long Unicode comment: Tiếng Việt — 日本語\nSecond line';
CREATE INDEX items_parent_idx ON release_check.items(tenant_id, parent_id);
INSERT INTO release_check.items
    (id, name, note, amount, payload, markup, raw, occurred_at)
VALUES
    (1, 'alpha', NULL, 1234567890123.45,
     '{"n":9007199254740993,"dup":1,"dup":2}', '<root>a<b/>c</root>',
     decode('00017fff','hex'), '2026-11-05T00:00:00Z'),
    (2, 'Tiếng Việt 日本語', '', 0, '{}', '<root/>',
     decode('','hex'), '2026-11-05T07:00:00+07:00'),
    (3, 'O''Reilly', E'comma, quote " and\nnew line', 42.10,
     NULL, NULL, NULL, NULL);
UPDATE release_check.items SET previous_id = 1 WHERE id = 2;
INSERT INTO release_check.items(id, name, note)
SELECT n, 'row-' || n, current_database() FROM generate_series(4,1000) AS n;

CREATE TABLE release_other.incoming (
    id integer PRIMARY KEY,
    item_id integer NOT NULL REFERENCES release_check.items(id)
);
INSERT INTO release_other.incoming VALUES (1,1);
CREATE TABLE release_check.empty_items (id integer PRIMARY KEY, note text);
CREATE TABLE release_check.editable (id integer PRIMARY KEY, value text NOT NULL);
INSERT INTO release_check.editable VALUES (1,'original'), (2,'second');
CREATE TABLE release_check.ddl_scratch (id integer PRIMARY KEY, note text);
CREATE TABLE release_check.import_target (id integer PRIMARY KEY, note text);
CREATE TABLE release_check.transfer_target (id integer PRIMARY KEY, note text);
CREATE VIEW release_check.items_view AS SELECT id, name FROM release_check.items;
CREATE MATERIALIZED VIEW release_check.items_materialized AS
    SELECT id, name FROM release_check.items WHERE id <= 3;
CREATE SEQUENCE release_check.extra_sequence;
CREATE TYPE release_check.status AS ENUM ('new','done');
CREATE FUNCTION release_check.echo(value text) RETURNS text
    LANGUAGE sql IMMUTABLE AS 'SELECT value';

CREATE TABLE release_check.events (id integer, happened_on date NOT NULL)
    PARTITION BY RANGE(happened_on);
CREATE TABLE release_check.events_2026 PARTITION OF release_check.events
    FOR VALUES FROM ('2026-01-01') TO ('2027-01-01');
CREATE TABLE release_check.guarded (id integer PRIMARY KEY, note text);
CREATE FUNCTION release_check.keep_row() RETURNS trigger LANGUAGE plpgsql AS
    'BEGIN RETURN NEW; END';
CREATE TRIGGER guarded_before_insert BEFORE INSERT ON release_check.guarded
    FOR EACH ROW EXECUTE FUNCTION release_check.keep_row();
ALTER TABLE release_check.guarded ENABLE ROW LEVEL SECURITY;
CREATE POLICY guarded_read ON release_check.guarded FOR SELECT USING (true);
CREATE RULE guarded_no_delete AS ON DELETE TO release_check.guarded DO INSTEAD NOTHING;

-- Expected baseline: 1000 / 3 / 1 / 2 / 0 / 0 / 0.
SELECT
    (SELECT count(*) FROM release_check.items) AS items,
    (SELECT count(*) FROM release_check.parents) AS parents,
    (SELECT count(*) FROM release_other.incoming) AS incoming,
    (SELECT count(*) FROM release_check.editable) AS editable,
    (SELECT count(*) FROM release_check.empty_items) AS empty_items,
    (SELECT count(*) FROM release_check.import_target) AS import_target,
    (SELECT count(*) FROM release_check.transfer_target) AS transfer_target;
