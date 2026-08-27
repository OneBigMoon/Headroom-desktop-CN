cask "headroom-local-community" do
  version "0.8.7"
  sha256 :no_check

  # Replace this URL with the Community project's own release asset before
  # publishing a tap. Never point Community updates at the paid app channel.
  url "https://example.invalid/headroom-local-community/Headroom_Local_Community_#{version}_aarch64.dmg"
  name "Headroom Local Community"
  desc "Unofficial local-only desktop shell for the open-source headroom-ai proxy"
  homepage "https://github.com/gglucass/headroom-desktop"

  depends_on macos: :sonoma
  depends_on arch: :arm64

  app "Headroom Local Community.app"

  uninstall launchctl: "org.headroomlocal.community",
            quit:      "org.headroomlocal.community"

  zap trash: [
    "~/.headroom-local-community",
    "~/Library/Application Support/HeadroomLocalCommunity",
    "~/Library/Caches/org.headroomlocal.community",
    "~/Library/HTTPStorages/org.headroomlocal.community",
    "~/Library/HTTPStorages/org.headroomlocal.community.binarycookies",
    "~/Library/LaunchAgents/org.headroomlocal.community.plist",
    "~/Library/LaunchAgents/HeadroomLocalCommunity.plist",
    "~/Library/Logs/HeadroomLocalCommunity",
    "~/Library/Preferences/org.headroomlocal.community.plist",
    "~/Library/Saved Application State/org.headroomlocal.community.savedState",
    "~/Library/WebKit/org.headroomlocal.community",
  ]
end
