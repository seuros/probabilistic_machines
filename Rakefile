# frozen_string_literal: true

require 'bundler/gem_tasks'
require 'minitest/test_task'
require 'shellwords'

Minitest::TestTask.create(:minitest) do |t|
  t.libs << 'test'
  t.libs << 'lib'
  t.test_globs = ['test/**/*_test.rb']
end

if RUBY_ENGINE == 'ruby'
  desc 'Run the test suite'
  task :test do
    sh 'padded_cell', *Shellwords.split(ENV.fetch('TESTOPTS', ''))
  end
else
  task test: :minitest
end

GEMSPEC = Gem::Specification.load('probabilistic_machines.gemspec')

require 'rb_sys/extensiontask'

SUPPORTED_NATIVE_PLATFORMS = %w[
  arm64-darwin
  x86_64-darwin
  aarch64-linux
  x86_64-linux
  x86_64-linux-musl
].freeze

native_ext = RbSys::ExtensionTask.new('probabilistic_machines_native', GEMSPEC) do |ext|
  ext.lib_dir = 'lib/probabilistic_machines_native'
  ext.tmp_dir = 'tmp/rb_sys'
  ext.cross_platform = SUPPORTED_NATIVE_PLATFORMS if ENV.key?('RUBY_TARGET')
end

# A single-version cross build stages its binary at the host binary's path,
# which the gem package depends on, so packaging would also compile for the
# host and fail where the host linker can't target the platform.
if ENV.key?('RUBY_TARGET')
  host_binary = "#{native_ext.lib_dir}/#{native_ext.name}.#{RbConfig::CONFIG['DLEXT']}"
  Rake::Task[host_binary].clear_prerequisites if Rake::Task.task_defined?(host_binary)
end

namespace :native do
  desc 'Compile the native extension in release mode for the current platform'
  task build: ['rb_sys:env:release', 'compile']

  desc 'Build native gems for all supported platforms'
  task :all do
    SUPPORTED_NATIVE_PLATFORMS.each do |platform|
      sh({ 'RUBY_TARGET' => platform }, 'bundle', 'exec', 'rake', 'native:build')
    end
  end
end

desc 'Run the Rust core crate tests'
task :cargo do
  sh 'cargo', 'test', '--manifest-path', 'ext/probabilistic_machines_native/core/Cargo.toml', '--all-features'
end

desc 'Lint the FFI crate'
task :clippy do
  sh 'cargo', 'clippy', '--manifest-path', 'ext/probabilistic_machines_native/ffi/Cargo.toml',
     '--all-targets', '--', '-D', 'warnings'
end

task test: :compile
task default: %i[test cargo]

Dir[File.join(__dir__, 'tasks/*.rake')].sort.each { |f| load f }
