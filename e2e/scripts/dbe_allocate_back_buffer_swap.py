import subprocess, sys
import os
# xdpyinfo should list DBE as a supported extension
result = subprocess.run(['xdpyinfo'], capture_output=True, text=True, env={**os.environ, 'DISPLAY': ':99'})
if 'DOUBLE-BUFFER' in result.stdout:
    print('dbe_supported_ok')
else:
    print('dbe_not_found')
print('done')
