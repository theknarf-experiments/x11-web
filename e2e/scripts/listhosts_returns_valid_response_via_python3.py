import Xlib.display
d = Xlib.display.Display()
hosts = d.list_hosts()
print(f"acl_enabled={hosts.mode}")
print(f"n_hosts={len(hosts.hosts)}")
d.close()
