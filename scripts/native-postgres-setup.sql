\set ON_ERROR_STOP on

DO $$ BEGIN
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'app_migrator') THEN
    CREATE ROLE app_migrator LOGIN PASSWORD 'migrator_dev_pw';
  END IF;
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'app_runtime') THEN
    CREATE ROLE app_runtime LOGIN PASSWORD 'runtime_dev_pw';
  END IF;
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'app_queue') THEN
    CREATE ROLE app_queue LOGIN PASSWORD 'queue_dev_pw';
  END IF;
END $$;

SELECT 'CREATE DATABASE brightcone_dev OWNER app_migrator'
WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = 'brightcone_dev')\gexec
SELECT 'CREATE DATABASE brightcone_test OWNER app_migrator'
WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = 'brightcone_test')\gexec

\connect brightcone_dev
REVOKE ALL ON DATABASE brightcone_dev FROM PUBLIC;
GRANT CONNECT ON DATABASE brightcone_dev TO app_runtime, app_queue;
GRANT USAGE, CREATE ON SCHEMA public TO app_migrator;
GRANT USAGE ON SCHEMA public TO app_runtime;
ALTER DEFAULT PRIVILEGES FOR ROLE app_migrator IN SCHEMA public
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO app_runtime;
ALTER DEFAULT PRIVILEGES FOR ROLE app_migrator IN SCHEMA public
  GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO app_runtime;
CREATE SCHEMA IF NOT EXISTS queue AUTHORIZATION app_queue;
GRANT USAGE, CREATE ON SCHEMA queue TO app_queue;
ALTER ROLE app_queue IN DATABASE brightcone_dev SET search_path = queue;

\connect brightcone_test
REVOKE ALL ON DATABASE brightcone_test FROM PUBLIC;
GRANT CONNECT ON DATABASE brightcone_test TO app_runtime, app_queue;
GRANT USAGE, CREATE ON SCHEMA public TO app_migrator;
GRANT USAGE ON SCHEMA public TO app_runtime;
ALTER DEFAULT PRIVILEGES FOR ROLE app_migrator IN SCHEMA public
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO app_runtime;
ALTER DEFAULT PRIVILEGES FOR ROLE app_migrator IN SCHEMA public
  GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO app_runtime;
CREATE SCHEMA IF NOT EXISTS queue AUTHORIZATION app_queue;
GRANT USAGE, CREATE ON SCHEMA queue TO app_queue;
ALTER ROLE app_queue IN DATABASE brightcone_test SET search_path = queue;
