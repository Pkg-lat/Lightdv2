#!/bin/sh
cd /home/container
exec /bin/sh -c "echo Container running && while true; do sleep 5; done"
