//! Low-level Unix socket I/O helpers with SCM_RIGHTS support.

use std::io;

/// Send data with file descriptors via SCM_RIGHTS (for SHM CreateSegment, etc.).
pub(crate) fn send_with_fds(sock_fd: i32, data: &[u8], fds: &[i32]) -> io::Result<usize> {
    unsafe {
        let mut iov = libc::iovec {
            iov_base: data.as_ptr() as *mut libc::c_void,
            iov_len: data.len(),
        };

        if fds.is_empty() {
            // No fds to send, just use normal sendmsg
            let mut msg: libc::msghdr = std::mem::zeroed();
            msg.msg_iov = &mut iov;
            msg.msg_iovlen = 1;
            let n = libc::sendmsg(sock_fd, &msg, 0);
            if n < 0 {
                return Err(io::Error::last_os_error());
            }
            return Ok(n as usize);
        }

        // Build ancillary data for SCM_RIGHTS
        let fd_bytes = std::mem::size_of_val(fds);
        let cmsg_space = libc::CMSG_SPACE(fd_bytes as u32) as usize;
        let mut cmsg_buf = vec![0u8; cmsg_space];

        let mut msg: libc::msghdr = std::mem::zeroed();
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = cmsg_buf.as_mut_ptr() as *mut libc::c_void;
        msg.msg_controllen = cmsg_space;

        let cmsg = libc::CMSG_FIRSTHDR(&msg);
        (*cmsg).cmsg_level = libc::SOL_SOCKET;
        (*cmsg).cmsg_type = libc::SCM_RIGHTS;
        (*cmsg).cmsg_len = libc::CMSG_LEN(fd_bytes as u32) as usize;
        let data_ptr = libc::CMSG_DATA(cmsg);
        std::ptr::copy_nonoverlapping(
            fds.as_ptr() as *const u8,
            data_ptr,
            fd_bytes,
        );

        let n = libc::sendmsg(sock_fd, &msg, 0);
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(n as usize)
    }
}

/// Try to receive data with optional SCM_RIGHTS file descriptors via recvmsg.
/// Returns (bytes_read, received_fds). Falls back to normal read if recvmsg fails.
pub(crate) fn recv_with_fds(fd: i32, buf: &mut [u8]) -> io::Result<(usize, Vec<i32>)> {
    unsafe {
        let mut iov = libc::iovec {
            iov_base: buf.as_mut_ptr() as *mut libc::c_void,
            iov_len: buf.len(),
        };

        // Space for up to 4 file descriptors in ancillary data
        let mut cmsg_buf = [0u8; 64];

        let mut msg: libc::msghdr = std::mem::zeroed();
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = cmsg_buf.as_mut_ptr() as *mut libc::c_void;
        msg.msg_controllen = cmsg_buf.len();

        let n = libc::recvmsg(fd, &mut msg, libc::MSG_DONTWAIT);
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::WouldBlock {
                return Err(err);
            }
            return Err(err);
        }

        let mut fds = Vec::new();
        // Parse ancillary data for SCM_RIGHTS
        let mut cmsg = libc::CMSG_FIRSTHDR(&msg);
        while !cmsg.is_null() {
            if (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
                let data_ptr = libc::CMSG_DATA(cmsg);
                let data_len = (*cmsg).cmsg_len - libc::CMSG_LEN(0) as usize;
                let num_fds = data_len / std::mem::size_of::<i32>();
                let fd_slice = std::slice::from_raw_parts(data_ptr as *const i32, num_fds);
                fds.extend_from_slice(fd_slice);
            }
            cmsg = libc::CMSG_NXTHDR(&msg, cmsg);
        }

        Ok((n as usize, fds))
    }
}
