using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Security.AccessControl;

// Exact owned-fixture DACL restoration; SetNamedSecurityInfo-based APIs can
// recompute inheritance and add AI or inherited ACEs to a legacy descriptor.
public static class DevboxReceiverAccess
{
    [DllImport("advapi32.dll", EntryPoint = "SetFileSecurityW", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool SetFileSecurity(string path, uint information, byte[] descriptor);

    public static void Restore(string path, string accessSddl)
    {
        var descriptor = new RawSecurityDescriptor(accessSddl);
        // The legacy API consumes AR+AI together to preserve an existing AI;
        // AR is a request bit, not a relaxation of the final equality check.
        if ((descriptor.ControlFlags & ControlFlags.DiscretionaryAclAutoInherited) != 0)
            descriptor.SetFlags(descriptor.ControlFlags | ControlFlags.DiscretionaryAclAutoInheritRequired);
        var bytes = new byte[descriptor.BinaryLength];
        descriptor.GetBinaryForm(bytes, 0);
        if (!SetFileSecurity(path, 4, bytes)) // DACL_SECURITY_INFORMATION only
            throw new Win32Exception(Marshal.GetLastWin32Error());
    }
}
