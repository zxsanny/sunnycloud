"""A gnome-keyring system prompter that accepts every call and never shows or answers a prompt.

Owns `org.gnome.keyring.SystemPrompter` on the private test bus, so an unlock of a locked keyring waits
forever: the "unlock prompt nobody answers" context of IT-14-18 (14 section 5). Prints `ready` once owned.
"""

import sys

from gi.repository import Gio, GLib

INTERFACE = """<node><interface name='org.gnome.keyring.internal.Prompter'>
<method name='BeginPrompting'><arg type='o' name='callback' direction='in'/></method>
<method name='PerformPrompt'><arg type='o' name='callback' direction='in'/><arg type='s' name='type' direction='in'/>
<arg type='a{sv}' name='properties' direction='in'/><arg type='s' name='exchange' direction='in'/></method>
<method name='StopPrompting'><arg type='o' name='callback' direction='in'/></method>
</interface></node>"""

INFO = Gio.DBusNodeInfo.new_for_xml(INTERFACE).interfaces[0]


def on_call(
    _connection: Gio.DBusConnection,
    _sender: str,
    _path: str,
    _interface: str,
    _method: str,
    _params: GLib.Variant,
    invocation: Gio.DBusMethodInvocation,
) -> None:
    invocation.return_value(None)


def on_acquired(connection: Gio.DBusConnection, _name: str) -> None:
    connection.register_object("/org/gnome/keyring/Prompter", INFO, on_call, None, None)
    print("ready", flush=True)


def on_lost(_connection: Gio.DBusConnection, _name: str) -> None:
    sys.exit(2)


Gio.bus_own_name(Gio.BusType.SESSION, "org.gnome.keyring.SystemPrompter", 0, on_acquired, None, on_lost)
GLib.MainLoop().run()
