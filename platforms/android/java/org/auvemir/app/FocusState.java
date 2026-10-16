package org.auvemir.app;

/** Main-thread focus policy. A transient pause retains its request until gain or explicit user transport. */
final class FocusState {
    boolean granted,requested;
    private boolean resume;
    void userTransport(){resume=false;}
    boolean gain(){if(!requested)return false;granted=true;boolean result=resume;resume=false;return result;}
    void loss(boolean transientLoss,boolean wasPlaying){granted=false;if(transientLoss){requested=true;resume=resume||wasPlaying;}else{requested=false;resume=false;}}
    void requestResult(boolean success){granted=success;requested=success;}
    boolean shouldRelease(){return requested&&!resume;}
    void released(){granted=false;requested=false;}
}
