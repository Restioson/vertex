#!/bin/sh
# wait-for-postgres.sh
# Modified from https://docs.docker.com/compose/startup-order/

set -e

host="$1"
port="$2"
shift 2
cmd="$@"

until PGPASSWORD=$POSTGRES_PASSWORD nc $host $port &> /dev/null; do
  >&2 echo "Postgres is unavailable - sleeping"
  sleep 2
done

>&2 echo "Postgres is up - executing command"
exec $cmd
