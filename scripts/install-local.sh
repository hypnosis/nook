#!/usr/bin/env bash
# Собирает Nook, подписывает и ставит в /Applications.
#
#   --adhoc              подпись ad-hoc, как в релизе
#   --reset-permissions  сбросить права Nook (после смены подписи)
#
# Без --adhoc подпись берётся из NOOK_SIGN_IDENTITY в .private/signing.env;
# если файла или переменной нет — ad-hoc.
set -euo pipefail

cd "$(dirname "$0")/.."

SIGNING_ENV=".private/signing.env"
APP="Nook.app"

adhoc=false
reset_permissions=false
for arg in "$@"; do
    case "${arg}" in
        --adhoc) adhoc=true ;;
        --reset-permissions) reset_permissions=true ;;
        *) echo "Unknown option: ${arg}" >&2; exit 1 ;;
    esac
done

NOOK_SIGN_IDENTITY=""
if [[ "${adhoc}" == false && -f "${SIGNING_ENV}" ]]; then
    # shellcheck source=/dev/null
    source "${SIGNING_ENV}"
fi

if [[ -n "${NOOK_SIGN_IDENTITY}" ]] && ! security find-identity -v -p codesigning | grep -qF "\"${NOOK_SIGN_IDENTITY}\""; then
    echo "Signing identity \"${NOOK_SIGN_IDENTITY}\" not found in keychain (${SIGNING_ENV})." >&2
    exit 1
fi

./bundle.sh
if [[ -n "${NOOK_SIGN_IDENTITY}" ]]; then
    codesign -s "${NOOK_SIGN_IDENTITY}" --force --deep "${APP}"
    signature="${NOOK_SIGN_IDENTITY}"
else
    signature="ad-hoc"
fi

osascript -e 'quit app "Nook"' || true
sleep 1
pkill -x nook || true

if [[ "${reset_permissions}" == true ]]; then
    bundle_id="$(/usr/libexec/PlistBuddy -c 'Print CFBundleIdentifier' "${APP}/Contents/Info.plist")"
    tccutil reset ScreenCapture "${bundle_id}"
    tccutil reset Accessibility "${bundle_id}"
fi

rm -rf "/Applications/${APP}"
cp -R "${APP}" "/Applications/${APP}"
open "/Applications/${APP}"
echo "==> Installed and launched /Applications/${APP} (signed: ${signature})"
