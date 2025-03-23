class Asimeow < Formula
  desc "A tool for managing macOS Time Machine exclusions for developer projects"
  homepage "https://github.com/mdnmdn/asimeow"
  version "0.1.1"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/mdnmdn/asimeow/releases/download/v#{version}/asimeow-aarch64-apple-darwin.zip"
      sha256 "REPLACE_WITH_ARM_BINARY_SHA256"
    elsif Hardware::CPU.intel?
      url "https://github.com/mdnmdn/asimeow/releases/download/v#{version}/asimeow-x86_64-apple-darwin.zip"
      sha256 "REPLACE_WITH_INTEL_BINARY_SHA256"
    end
  end

  # Fallback to source code if no binary is available
  resource "source" do
    url "https://github.com/mdnmdn/asimeow/archive/refs/tags/v#{version}.tar.gz"
    sha256 "REPLACE_WITH_SOURCE_SHA256"
  end

  depends_on "rust" => :build

  def install
    if Hardware::CPU.arm? || Hardware::CPU.intel?
      # Binary installation
      bin.install "asimeow"
    else
      # Source installation as fallback
      resource("source").stage do
        system "cargo", "install", "--locked", "--root", prefix, "--path", "."
      end
    end

    # Install the launchd plist file
    (prefix/"homebrew.mxcl.asimeow.plist").write launchd_plist
  end

  def launchd_plist
    <<~EOS
      <?xml version="1.0" encoding="UTF-8"?>
      <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
      <plist version="1.0">
          <dict>
              <key>Label</key>
              <string>homebrew.mxcl.asimeow</string>
              <key>Program</key>
              <string>#{opt_bin}/asimeow</string>
              <key>StartCalendarInterval</key>
             <dict>
               <key>Hour</key>
               <integer>23</integer>
               <key>Minute</key>
               <integer>11</integer>
             </dict>
          </dict>
      </plist>
    EOS
  end

  def caveats
    <<~EOS
      To have launchd start asimeow now and restart at login:
        brew services start asimeow
      
      Or, if you don't want/need a background service you can just run:
        asimeow
    EOS
  end

  test do
    system "#{bin}/asimeow", "--version"
  end
end