import subprocess
# Use xdpyinfo to verify SECURITY is listed
result = subprocess.run(['xdpyinfo'], capture_output=True, text=True, env={'DISPLAY': ':99'})
if 'SECURITY' in result.stdout:
    print('security_supported_ok')
else:
    print('security_not_found')
print('done')
