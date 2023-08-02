#!/bin/bash

. ./observer/build_all.sh

# The above script will navigate us to the observer directory
foreman start -f Procfile
