-- Product-authored read-only export recipe for an OPERATOR-SELECTED ISOLATED
-- restored PostgreSQL snapshot. Never run this against a live source by default.
-- psql -X --no-password -v ON_ERROR_STOP=1 -v legacy_owner=... \
--   -v deployment_id=... -v source_version=... -f schema23-relations.sql ...
-- Redirect stdout to a private file under umask077. No credential belongs in argv.
\set ON_ERROR_STOP on
\set QUIET on
\pset format unaligned
\pset tuples_only on
\pset footer off
\if :{?legacy_owner}
\else
  \warn legacy_owner_required
  \quit 3
\endif
\if :{?deployment_id}
\else
  \warn deployment_id_required
  \quit 3
\endif
\if :{?source_version}
\else
  \warn source_version_required
  \quit 3
\endif
BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;
SET LOCAL search_path = pg_catalog;
SELECT jsonb_build_object('type','header','format',1,
 'purpose','musuri-schema23-relations-v1','source_schema',23,
 'owner',:'legacy_owner','deployment_id',:'deployment_id','source_version',:'source_version',
 'snapshot',txid_current_snapshot()::text,'exported_at',current_timestamp,
 'inventory',coalesce(jsonb_agg(jsonb_build_object('schema',n.nspname,'name',c.relname)
     ORDER BY n.nspname,c.relname),'[]'::jsonb))
FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
WHERE c.relkind IN ('r','p') AND left(n.nspname, 3) <> 'pg_'
 AND n.nspname <> 'information_schema';
-- Catalog names use format %I identifiers and %L values. Only SELECT is generated.
SELECT format('SELECT jsonb_build_object(''type'',''relation'',''schema'',%L,''name'',%L,''count'',count(*),''rows'',coalesce(jsonb_agg(to_jsonb(t)),''[]''::jsonb)) FROM ONLY %I.%I t;',
 n.nspname,c.relname,n.nspname,c.relname)
FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
WHERE c.relkind IN ('r','p') AND left(n.nspname, 3) <> 'pg_'
 AND n.nspname <> 'information_schema'
ORDER BY n.nspname,c.relname
\gexec
SELECT jsonb_build_object('type','complete','snapshot',txid_current_snapshot()::text,
 'relations',count(*))
FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
WHERE c.relkind IN ('r','p') AND left(n.nspname, 3) <> 'pg_'
 AND n.nspname <> 'information_schema';
COMMIT;
