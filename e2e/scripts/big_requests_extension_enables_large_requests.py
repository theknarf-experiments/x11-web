import Xlib.display
d = Xlib.display.Display()
bigreq = d.query_extension('BIG-REQUESTS')
print(f"bigreq_present={bigreq is not None and bigreq.major_opcode > 0}")

# Max request length should be > 65535 after enabling big-requests
max_len = d.info.max_request_length
print(f"max_request_length={max_len}")
print(f"big_requests_work={max_len > 65535}")
d.close()
