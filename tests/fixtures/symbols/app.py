# Concern: demo module exercising the Python symbol extractor | Non-concern: real behavior (a fixture stub) | IO: (Settings) -> App
import os


def build_app(settings):
    return settings


class Server:
    def start(self):
        return True

    def stop(self):
        return False
