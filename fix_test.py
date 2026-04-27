import subprocess

def run_test():
    result = subprocess.run(["cargo", "test", "-p", "tsuzulint_plugin", "--test", "security_limits", "--features", "test-utils"], capture_output=True, text=True)
    print(result.stdout)
    print(result.stderr)

run_test()
