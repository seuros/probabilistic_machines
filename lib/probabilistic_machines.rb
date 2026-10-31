# frozen_string_literal: true

require_relative 'probabilistic_machines/version'

# Every class, the two `Data` result objects and both error classes are defined
# by the native extension; there is no pure-Ruby implementation to fall back to.
require 'probabilistic_machines_native/probabilistic_machines_native'
