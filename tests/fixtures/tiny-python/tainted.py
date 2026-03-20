import subprocess
import sys


def run_command(user_input):
    subprocess.call(user_input, shell=True)  # CWE-78: tainted input to shell


if __name__ == "__main__":
    run_command(sys.argv[1])
