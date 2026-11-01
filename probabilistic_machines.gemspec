# frozen_string_literal: true

require_relative 'lib/probabilistic_machines/version'

Gem::Specification.new do |spec|
  spec.name = 'probabilistic_machines'
  spec.version = ProbabilisticMachines::VERSION
  spec.authors = ['Abdelkader Boudih']
  spec.email = ['oss@seuros.com']

  spec.summary = 'Distinct counts, quantiles and frequencies in bounded memory'
  spec.description = 'HyperLogLog, Count-Min Sketch, t-digest, cuckoo filter and Space-Saving top-k, ' \
                     'bound to Ruby from the no_std probabilistic-machines crate. Each sketch answers ' \
                     'a counting question in memory fixed at construction time, and serialises to a ' \
                     'byte string other languages reading the same crate can decode.'
  spec.homepage = 'https://github.com/seuros/probabilistic_machines'
  spec.license = 'MIT'
  spec.required_ruby_version = '>= 4.0.0'

  spec.metadata['source_code_uri'] = spec.homepage
  spec.metadata['changelog_uri'] = "#{spec.homepage}/blob/main/CHANGELOG.md"
  spec.metadata['bug_tracker_uri'] = "#{spec.homepage}/issues"
  spec.metadata['rubygems_mfa_required'] = 'true'
  spec.metadata['cargo_crate_name'] = 'probabilistic_machines_native'
  spec.metadata['cargo_manifest_path'] = 'ext/probabilistic_machines_native/ffi/Cargo.toml'

  spec.files = Dir.glob(%w[
                          lib/**/*.rb
                          sig/**/*.rbs
                          ext/**/*.{rb,rs,toml}
                          Cargo.toml
                          Cargo.lock
                          README.md
                          CHANGELOG.md
                          LICENSE.txt
                        ]).select { |path| File.exist?(path) }
                   .reject { |path| path =~ %r{/tests(/|\.rs\z)} }
  spec.require_paths = ['lib']
  spec.extensions = ['ext/probabilistic_machines_native/extconf.rb']

  # A source install runs extconf.rb, which requires rb_sys/mkmf, and RubyGems
  # clears GEM_PATH while building the extension. Without this the build only
  # works where rb_sys happens to be installed globally.
  spec.add_dependency 'rb_sys', '~> 0.9'
end
