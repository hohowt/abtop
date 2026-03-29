class Abtop < Formula
  desc "AI agent monitor for your terminal"
  homepage "https://github.com/graykode/abtop"
  version "{VERSION}"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/graykode/abtop/releases/download/v{VERSION}/abtop-aarch64-apple-darwin.tar.gz"
      sha256 "{SHA256_ARM}"
    end

    on_intel do
      url "https://github.com/graykode/abtop/releases/download/v{VERSION}/abtop-x86_64-apple-darwin.tar.gz"
      sha256 "{SHA256_INTEL}"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/graykode/abtop/releases/download/v{VERSION}/abtop-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "{SHA256_LINUX_ARM}"
    end

    on_intel do
      url "https://github.com/graykode/abtop/releases/download/v{VERSION}/abtop-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "{SHA256_LINUX_INTEL}"
    end
  end

  def install
    bin.install "abtop"
  end

  test do
    assert_match "abtop", shell_output("#{bin}/abtop --help 2>&1", 0)
  end
end
