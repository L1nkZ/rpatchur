#!/usr/bin/env bash

set -ex

if [ "$TRAVIS_OS_NAME" != linux ]; then
    exit 0
fi

sudo apt-get update
sudo apt-get install -y libsoup2.4-dev libgtk-3-dev libwebkit2gtk-4.0-dev
