#!/bin/bash
SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd)

if [[ ! -d "${SCRIPT_DIR}/.venv" ]]; then
    echo "Creating virtual environment..."
    python3 -m venv "${SCRIPT_DIR}/.venv"
    source "${SCRIPT_DIR}/.venv/bin/activate"
    if [[ -f "${SCRIPT_DIR}/requirements.txt" ]]; then
        echo "Installing requirements..."
        pip install -q -r "${SCRIPT_DIR}/requirements.txt"
    fi
else
    source "${SCRIPT_DIR}/.venv/bin/activate"
fi

python3 "${SCRIPT_DIR}/cartouche.py" "$@"
