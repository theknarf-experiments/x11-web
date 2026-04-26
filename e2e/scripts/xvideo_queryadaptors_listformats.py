import subprocess, re
import os
out = subprocess.check_output(['xvinfo'], env={**os.environ, 'DISPLAY': ':99'}).decode()
print(out[:2000])
# Count advertised formats
fmts = re.findall(r'id:\s+0x[0-9a-fA-F]+', out)
print(f'format-count={len(fmts)}')
assert len(fmts) >= 8, f'Expected >=8 formats, got {len(fmts)}'
# Check for NV12 and YUY2
assert 'YUY2' in out or 'yuy2' in out.lower(), 'Missing YUY2'
print('PASS: XVideo formats advertised')
