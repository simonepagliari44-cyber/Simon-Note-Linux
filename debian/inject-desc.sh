#!/bin/sh
# Inietta una descrizione tradotta dentro un pacchetto .deb già costruito.
#
# dpkg-gencontrol non accetta i campi "Description-xx": vengono scartati con
# un avviso. L'unico modo per avere una descrizione localizzata in un .deb
# locale è scriverla a mano nel file DEBIAN/control dopo la build.
#
# Uso: inject-desc.sh <pacchetto.deb> <file-con-la-descrizione>
set -e

DEB="$1"
DESC="$2"

[ -f "$DEB" ] || { echo "inject-desc: pacchetto non trovato: $DEB" >&2; exit 1; }
[ -f "$DESC" ] || { echo "inject-desc: descrizione non trovata: $DESC" >&2; exit 1; }

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

# decomprime, aggiunge il campo, ricomprime
dpkg-deb --raw-extract "$DEB" "$TMP"
cat "$DESC" >> "$TMP/DEBIAN/control"
dpkg-deb --build --root-owner-group "$TMP" "$DEB" >/dev/null

echo "inject-desc: descrizione da $(basename "$DESC") aggiunta a $(basename "$DEB")"
